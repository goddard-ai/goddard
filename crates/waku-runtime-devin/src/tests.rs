use super::*;
use serde_json::json;
use std::sync::mpsc::{Receiver, sync_channel};
use waku_runtime::contract::{
    ActionEnvelope, ClientHello, CommandDisposition, EventCursor, RequestedConfig, SessionRef,
    TerminalOutcome,
};
use waku_runtime::fake::ManualClock;
use waku_runtime::host::{HostOptions, RuntimeHost};

struct Helper(std::process::Child);
impl Drop for Helper {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

struct Fixture {
    root: PathBuf,
    binary: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        use std::os::unix::fs::PermissionsExt;
        let root = std::env::temp_dir().join(format!("runtime-devin-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let binary = root.join("devin");
        std::fs::write(&binary, include_str!("fixture.py")).unwrap();
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
        Self { root, binary }
    }
    fn spawn(&self, capacity: usize) -> (Box<dyn AdapterHandle>, Receiver<ProviderEvent>) {
        let (sink, rx) = sync_channel(capacity);
        let handle = DevinFactory::new(self.binary.clone())
            .spawn(SessionContext {
                session_id: Uuid::new_v4(),
                generation: 1,
                namespace: "devin-test".into(),
                adapter: "devin".into(),
                launch: json!({"cwd":self.root}),
                requested: RequestedConfig::default(),
                sink,
            })
            .unwrap();
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(10)).unwrap(),
            ProviderEvent::SessionLive
        );
        assert!(matches!(
            rx.recv_timeout(Duration::from_secs(10)).unwrap(),
            ProviderEvent::ConfigActivated { .. }
        ));
        (handle, rx)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
fn recv(rx: &Receiver<ProviderEvent>) -> ProviderEvent {
    rx.recv_timeout(Duration::from_secs(10)).unwrap()
}
fn submit(text: &str) -> CanonicalAction {
    CanonicalAction::Submit {
        turn_id: Uuid::new_v4(),
        delivery_id: Uuid::new_v4(),
        input: json!(text),
    }
}
fn permission(rx: &Receiver<ProviderEvent>) -> String {
    loop {
        match recv(rx) {
            ProviderEvent::RequestOpened { request_id, .. } => return request_id,
            ProviderEvent::ProviderFailed { detail } => panic!("{detail}"),
            ProviderEvent::Terminated { reason } => panic!("{reason:?}"),
            _ => {}
        }
    }
}

#[test]
fn original_responder_survives_controller_absence_and_consumption_is_not_fabricated() {
    let fixture = Fixture::new();
    let (mut handle, rx) = fixture.spawn(16);
    let action = submit("wait for approval");
    assert!(matches!(
        handle.dispatch(&action),
        DispatchOutcome::Unknown { .. }
    ));
    let id = permission(&rx);
    // The original provider waits at this explicit barrier; no controller is
    // required to retain its SDK connection, request ID, or responder.
    assert!(matches!(
        handle.dispatch(&CanonicalAction::ResolveRequest {
            request_id: id.clone(),
            resolution: json!({"optionId":"foreign"})
        }),
        DispatchOutcome::Rejected { .. }
    ));
    assert!(matches!(
        handle.dispatch(&CanonicalAction::ResolveRequest {
            request_id: id.clone(),
            resolution: json!({"optionId":"allow"})
        }),
        DispatchOutcome::Unknown { .. }
    ));
    assert!(matches!(recv(&rx), ProviderEvent::TurnCompleted { .. }));
    assert!(rx.try_recv().is_err()); // no false RequestAnswered
    assert!(matches!(
        handle.dispatch(&CanonicalAction::ResolveRequest {
            request_id: id,
            resolution: json!({"optionId":"allow"})
        }),
        DispatchOutcome::Rejected { .. }
    ));
    let _ = handle.terminate();
    assert!(matches!(
        recv(&rx),
        ProviderEvent::Terminated {
            reason: TerminateReason::Requested
        }
    ));
    assert_eq!(handle.terminate(), Termination::Confirmed);
}

#[test]
fn cancel_names_the_original_turn_and_waits_for_native_cancelled_response() {
    let fixture = Fixture::new();
    let (mut handle, rx) = fixture.spawn(16);
    let action = submit("wait for approval");
    let CanonicalAction::Submit { turn_id, .. } = action else {
        unreachable!()
    };
    handle.dispatch(&action);
    let id = permission(&rx);
    assert_eq!(handle.cancel(Uuid::new_v4()), CancelOutcome::NotRunning);
    assert!(matches!(
        handle.cancel(turn_id),
        CancelOutcome::Unknown { .. }
    ));
    assert_eq!(recv(&rx), ProviderEvent::TurnCancelled { turn_id });
    assert!(matches!(
        handle.dispatch(&CanonicalAction::ResolveRequest {
            request_id: id,
            resolution: json!({"optionId":"allow"})
        }),
        DispatchOutcome::Rejected { .. }
    ));
    handle.terminate();
    recv(&rx);
}

#[test]
fn full_intake_interrupts_explicitly_without_blocking_acp_responses() {
    let fixture = Fixture::new();
    let (mut handle, rx) = fixture.spawn(2);
    handle.dispatch(&submit("flood"));
    // The actor emits acceptance then deltas without awaiting this consumer.
    // Wait for its explicit stop barrier instead of a scheduling sleep.
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while handle.alive() {
        assert!(std::time::Instant::now() < deadline);
        std::thread::yield_now();
    }
    let mut terminal = false;
    while let Ok(event) = rx.recv_timeout(Duration::from_secs(10)) {
        if matches!(
            event,
            ProviderEvent::Terminated {
                reason: TerminateReason::Backpressure
            }
        ) {
            terminal = true;
            break;
        }
    }
    assert!(terminal);
}

#[test]
fn host_controller_reload_storm_replays_one_live_turn_without_redispatch() {
    let fixture = Fixture::new();
    let mut host = RuntimeHost::open(
        &fixture.root.join("host"),
        HostOptions::new("devin-test"),
        Arc::new(DevinFactory::new(fixture.binary.clone())),
        Box::new(ManualClock::new(1_000_000)),
    )
    .unwrap();
    let credential = host.credentials().controller.clone();
    let mut grant = host
        .acquire_control(&credential, &ClientHello::current())
        .unwrap();
    let session_id = Uuid::new_v4();
    let session = SessionRef {
        session_id,
        generation: 1,
    };
    let envelope =
        |host: &RuntimeHost, grant: &waku_runtime::contract::ControlGrant, id, session, action| {
            ActionEnvelope {
                namespace: "devin-test".into(),
                runtime_instance: host.instance(),
                epoch: grant.epoch,
                retry_window_id: grant.retry_window_id,
                command_id: id,
                session,
                action,
            }
        };
    host.execute(
        &credential,
        envelope(
            &host,
            &grant,
            Uuid::new_v4(),
            None,
            CanonicalAction::AdmitSession {
                session_id,
                adapter: "devin".into(),
                launch: json!({"cwd": fixture.root}),
                requested: RequestedConfig::default(),
            },
        ),
    )
    .unwrap();
    let input = submit("wait for approval");
    let command_id = Uuid::new_v4();
    let original = envelope(&host, &grant, command_id, Some(session), input.clone());
    assert!(matches!(
        host.execute(&credential, original.clone())
            .unwrap()
            .disposition,
        CommandDisposition::Uncertain { .. }
    ));
    let cursor = EventCursor {
        session_id,
        generation: 1,
        applied: 0,
    };
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    let id = loop {
        host.pump();
        let page = host.events(&credential, grant.epoch, cursor).unwrap();
        if let Some(id) = page.events.iter().find_map(|e| match &e.event {
            ProviderEvent::RequestOpened { request_id, .. } => Some(request_id.clone()),
            _ => None,
        }) {
            break id;
        }
        assert!(std::time::Instant::now() < deadline);
        std::thread::yield_now();
    };
    let initial = host.events(&credential, grant.epoch, cursor).unwrap();
    for _ in 0..25 {
        grant = host
            .acquire_control(&credential, &ClientHello::current())
            .unwrap();
        let inventory = host.reconcile(&credential, grant.epoch).unwrap();
        assert!(inventory.sessions[0].adapter_alive);
        host.execute(
            &credential,
            envelope(&host, &grant, command_id, Some(session), input.clone()),
        )
        .unwrap();
        assert_eq!(
            host.events(&credential, grant.epoch, cursor).unwrap(),
            initial
        );
    }
    let resolve_id = Uuid::new_v4();
    host.execute(
        &credential,
        envelope(
            &host,
            &grant,
            resolve_id,
            Some(session),
            CanonicalAction::ResolveRequest {
                request_id: id,
                resolution: json!({"optionId":"allow"}),
            },
        ),
    )
    .unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        host.pump();
        if host
            .command(&credential, command_id)
            .unwrap()
            .disposition
            .is_terminal()
        {
            break;
        }
        assert!(std::time::Instant::now() < deadline);
        std::thread::yield_now();
    }
    let events = host
        .events(&credential, grant.epoch, cursor)
        .unwrap()
        .events;
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e.event, ProviderEvent::TurnStarted { .. }))
            .count(),
        1
    );
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e.event, ProviderEvent::TurnCompleted { .. }))
            .count(),
        1
    );
    assert!(
        events
            .windows(2)
            .all(|pair| pair[1].sequence == pair[0].sequence + 1)
    );
    assert!(matches!(
        host.command(&credential, command_id).unwrap().disposition,
        CommandDisposition::Terminal {
            outcome: TerminalOutcome::Completed,
            ..
        }
    ));
    assert!(matches!(
        host.command(&credential, resolve_id).unwrap().disposition,
        CommandDisposition::Uncertain { .. }
    ));
    assert_eq!(
        std::fs::read_to_string(fixture.root.join("prompts")).unwrap(),
        "1\n"
    );
    host.execute(
        &credential,
        envelope(
            &host,
            &grant,
            Uuid::new_v4(),
            Some(session),
            CanonicalAction::StopSession,
        ),
    )
    .unwrap();
}

#[test]
fn runtime_death_fences_only_its_owned_provider_group() {
    if let Some(root) = std::env::var_os("RUNTIME_DEVIN_GUARDIAN_CHILD") {
        let root = PathBuf::from(root);
        let (sink, rx) = sync_channel(16);
        let _handle = DevinFactory::new(root.join("devin"))
            .spawn(SessionContext {
                session_id: Uuid::new_v4(),
                generation: 1,
                namespace: "guardian-child".into(),
                adapter: "devin".into(),
                launch: json!({"cwd":root}),
                requested: RequestedConfig::default(),
                sink,
            })
            .unwrap();
        assert_eq!(recv(&rx), ProviderEvent::SessionLive);
        let ProviderEvent::ConfigActivated { effective, .. } = recv(&rx) else {
            panic!("missing owned PID");
        };
        println!(
            "DEVIN_GUARD_READY {}",
            effective.options["ownedPid"].as_u64().unwrap()
        );
        use std::io::Write;
        std::io::stdout().flush().unwrap();
        loop {
            std::thread::park();
        }
    }
    use std::io::BufRead;
    let fixture = Fixture::new();
    let mut helper = Helper(
        std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "tests::runtime_death_fences_only_its_owned_provider_group",
                "--nocapture",
            ])
            .env("RUNTIME_DEVIN_GUARDIAN_CHILD", &fixture.root)
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let reader = std::io::BufReader::new(helper.0.stdout.take().unwrap());
    let pid = reader
        .lines()
        .map(Result::unwrap)
        .find_map(|line| {
            line.split_once("DEVIN_GUARD_READY ")
                .map(|(_, pid)| pid.trim().parse::<i32>().unwrap())
        })
        .expect("disposable runtime child ready barrier");
    helper.0.kill().unwrap();
    helper.0.wait().unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        // A failed kill(-pgid, 0) probe (including EPERM) does not establish
        // group absence. Observe membership instead of inferring from errno.
        let inventory = std::process::Command::new("/bin/ps")
            .args(["-axo", "pid=,pgid=,stat="])
            .output()
            .unwrap();
        assert!(inventory.status.success());
        let active = String::from_utf8(inventory.stdout)
            .unwrap()
            .lines()
            .any(|line| {
                let mut fields = line.split_whitespace();
                let _pid = fields.next();
                let group = fields.next().and_then(|group| group.parse::<i32>().ok());
                let status = fields.next().unwrap_or("");
                group == Some(pid) && !status.starts_with('Z')
            });
        if !active {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "runtime guardian left its provider group alive"
        );
    }
}

#[test]
fn lost_acceptance_never_becomes_completion_or_automatic_replay() {
    let fixture = Fixture::new();
    let (mut handle, rx) = fixture.spawn(16);
    assert!(matches!(
        handle.dispatch(&submit("lose-connection")),
        DispatchOutcome::Unknown { .. }
    ));
    loop {
        match recv(&rx) {
            ProviderEvent::TurnStarted { .. } | ProviderEvent::TurnCompleted { .. } => {
                panic!("fabricated provider acceptance/outcome")
            }
            ProviderEvent::Terminated {
                reason: TerminateReason::Interrupted { .. },
            } => break,
            _ => {}
        }
    }
    assert!(!handle.alive());
    assert!(matches!(
        handle.dispatch(&submit("retry")),
        DispatchOutcome::Rejected { .. }
    ));
    assert_eq!(
        std::fs::read_to_string(fixture.root.join("prompts")).unwrap(),
        "1\n"
    );
}

#[test]
fn provider_refusal_and_exhausted_budgets_never_report_success() {
    for stop_reason in ["refusal", "max_tokens", "max_turn_requests"] {
        let fixture = Fixture::new();
        let (mut handle, rx) = fixture.spawn(16);
        assert!(matches!(
            handle.dispatch(&submit(stop_reason)),
            DispatchOutcome::Unknown { .. }
        ));
        let mut failed = false;
        loop {
            match recv(&rx) {
                ProviderEvent::TurnCompleted { .. } => {
                    panic!("unsuccessful native stop presented as completion")
                }
                ProviderEvent::ProviderFailed { .. } => failed = true,
                ProviderEvent::Terminated {
                    reason: TerminateReason::Interrupted { .. },
                } => break,
                _ => {}
            }
        }
        assert!(failed);
    }
}

#[test]
fn recovery_cursor_cannot_fall_through_to_a_fresh_session() {
    let fixture = Fixture::new();
    let (sink, _rx) = sync_channel(1);
    let result = DevinFactory::new(fixture.binary.clone()).spawn(SessionContext {
        session_id: Uuid::new_v4(),
        generation: 1,
        namespace: "devin-test".into(),
        adapter: "devin".into(),
        launch: json!({"cwd":fixture.root, "resumeSessionId":"old-session"}),
        requested: RequestedConfig::default(),
        sink,
    });
    assert!(result.is_err());
    assert!(!fixture.root.join("prompts").exists());
}

#[test]
fn oversized_acp_record_is_rejected_before_json_allocation() {
    let data = vec![b'x'; MAX_LINE_BYTES + 1];
    let mut reader = futures::io::Cursor::new(data);
    let error = smol::block_on(bounded_line(&mut reader)).unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
}

#[test]
#[ignore = "requires installed authenticated Devin; disposable real provider session, billable"]
fn installed_devin_streams_on_original_runtime_transport() {
    let fixture = Fixture::new();
    let factory = DevinFactory::new(PathBuf::from("/opt/homebrew/bin/devin"));
    let mut host = RuntimeHost::open(
        &fixture.root.join("actual-host"),
        HostOptions::new("devin-live-test"),
        Arc::new(factory),
        Box::new(ManualClock::new(1_000_000)),
    )
    .unwrap();
    let credential = host.credentials().controller.clone();
    let mut grant = host
        .acquire_control(&credential, &ClientHello::current())
        .unwrap();
    let session_id = Uuid::new_v4();
    let session = SessionRef {
        session_id,
        generation: 1,
    };
    let envelope =
        |host: &RuntimeHost, grant: &waku_runtime::contract::ControlGrant, id, target, action| {
            ActionEnvelope {
                namespace: "devin-live-test".into(),
                runtime_instance: host.instance(),
                epoch: grant.epoch,
                retry_window_id: grant.retry_window_id,
                command_id: id,
                session: target,
                action,
            }
        };
    host.execute(
        &credential,
        envelope(
            &host,
            &grant,
            Uuid::new_v4(),
            None,
            CanonicalAction::AdmitSession {
                session_id,
                adapter: "devin".into(),
                launch: json!({"cwd":fixture.root}),
                requested: RequestedConfig::default(),
            },
        ),
    )
    .expect("actual Devin admission");
    let cursor = EventCursor {
        session_id,
        generation: 1,
        applied: 0,
    };
    let command_id = Uuid::new_v4();
    let input =
        submit("Reply with exactly STABLE_RUNTIME_DEVIN_OK. Do not run tools or modify any files.");
    host.execute(
        &credential,
        envelope(&host, &grant, command_id, Some(session), input.clone()),
    )
    .unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(120);
    loop {
        let page = host.events(&credential, grant.epoch, cursor).unwrap();
        assert!(
            !page.events.iter().any(|e| matches!(
                e.event,
                ProviderEvent::ProviderFailed { .. } | ProviderEvent::RequestOpened { .. }
            )),
            "actual Devin failed or unexpectedly requested approval"
        );
        if page
            .events
            .iter()
            .any(|e| matches!(e.event, ProviderEvent::TurnStarted { .. }))
        {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "actual Devin acceptance deadline"
        );
        std::thread::yield_now();
    }
    let instance = host.instance();
    for _ in 0..25 {
        grant = host
            .acquire_control(&credential, &ClientHello::current())
            .unwrap();
        let inventory = host.reconcile(&credential, grant.epoch).unwrap();
        assert_eq!(inventory.runtime_instance, instance);
        assert!(inventory.sessions[0].adapter_alive);
        host.execute(
            &credential,
            envelope(&host, &grant, command_id, Some(session), input.clone()),
        )
        .unwrap();
    }
    loop {
        host.pump();
        if host
            .command(&credential, command_id)
            .unwrap()
            .disposition
            .is_terminal()
        {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "actual Devin completion deadline"
        );
        std::thread::yield_now();
    }
    let events = host
        .events(&credential, grant.epoch, cursor)
        .unwrap()
        .events;
    let native_session = events
        .iter()
        .find_map(|e| match &e.event {
            ProviderEvent::ConfigActivated { effective, .. } => effective
                .options
                .get("nativeSessionId")
                .and_then(Value::as_str),
            _ => None,
        })
        .expect("journaled native session identity");
    let owned_pid = events
        .iter()
        .find_map(|e| match &e.event {
            ProviderEvent::ConfigActivated { effective, .. } => {
                effective.options.get("ownedPid").and_then(Value::as_u64)
            }
            _ => None,
        })
        .unwrap();
    eprintln!(
        "Actual Devin evidence: runtime={instance}, session={session_id}, generation=1, nativeSession={native_session}, ownedPid={owned_pid}, controllerTransfers=25"
    );
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e.event, ProviderEvent::TurnStarted { .. }))
            .count(),
        1
    );
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e.event, ProviderEvent::TurnCompleted { .. }))
            .count(),
        1
    );
    assert!(
        events
            .iter()
            .any(|e| matches!(e.event, ProviderEvent::TurnDelta { .. }))
    );
    assert!(
        events
            .windows(2)
            .all(|pair| pair[1].sequence == pair[0].sequence + 1)
    );
    assert!(matches!(
        host.command(&credential, command_id).unwrap().disposition,
        CommandDisposition::Terminal {
            outcome: TerminalOutcome::Completed,
            ..
        }
    ));
    let stop_id = Uuid::new_v4();
    host.execute(
        &credential,
        envelope(
            &host,
            &grant,
            stop_id,
            Some(session),
            CanonicalAction::StopSession,
        ),
    )
    .unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        host.pump();
        if host
            .command(&credential, stop_id)
            .unwrap()
            .disposition
            .is_terminal()
        {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "actual Devin child exit deadline"
        );
        std::thread::yield_now();
    }
    assert!(
        host.events(&credential, grant.epoch, cursor)
            .unwrap()
            .events
            .iter()
            .any(|e| matches!(
                e.event,
                ProviderEvent::Terminated {
                    reason: TerminateReason::Requested
                }
            ))
    );
}
