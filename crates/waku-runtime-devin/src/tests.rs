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
fn original_free_form_responder_validates_and_preserves_wire_answer() {
    let fixture = Fixture::new();
    let (mut handle, rx) = fixture.spawn(16);
    handle.dispatch(&submit("input"));
    let id = permission(&rx);
    assert!(matches!(
        handle.dispatch(&CanonicalAction::ResolveRequest {
            request_id: id.clone(),
            resolution: json!({"action":"accept","content":{"label":42}})
        }),
        DispatchOutcome::Rejected { .. }
    ));
    let response = json!({"action":"accept", "content":{"label":"release 42\nsecond line"}});
    assert!(matches!(
        handle.dispatch(&CanonicalAction::ResolveRequest {
            request_id: id.clone(),
            resolution: response.clone()
        }),
        DispatchOutcome::Unknown { .. }
    ));
    assert!(matches!(recv(&rx), ProviderEvent::TurnCompleted { .. }));
    assert_eq!(
        serde_json::from_str::<Value>(
            &std::fs::read_to_string(fixture.root.join("input-answer")).unwrap()
        )
        .unwrap(),
        response
    );
    assert!(rx.try_recv().is_err());
    assert!(matches!(
        handle.dispatch(&CanonicalAction::ResolveRequest {
            request_id: id,
            resolution: response
        }),
        DispatchOutcome::Rejected { .. }
    ));
    let action = submit("input");
    let CanonicalAction::Submit { turn_id, .. } = action else {
        unreachable!()
    };
    handle.dispatch(&action);
    let id = permission(&rx);
    // Cancel must quiesce free-form responders as well as permissions.
    handle.cancel(turn_id);
    assert_eq!(recv(&rx), ProviderEvent::TurnCancelled { turn_id });
    assert!(matches!(
        handle.dispatch(&CanonicalAction::ResolveRequest {
            request_id: id,
            resolution: json!({"action":"cancel"})
        }),
        DispatchOutcome::Rejected { .. }
    ));
    handle.terminate();
    recv(&rx);
}

#[test]
fn revision_activation_requires_advertised_values_and_provider_confirmation() {
    let fixture = Fixture::new();
    let (mut handle, rx) = fixture.spawn(16);
    let target = RequestedConfig {
        model: Some("model-b".into()),
        effort: Some("high".into()),
        ..Default::default()
    };
    assert!(matches!(
        handle.dispatch(&CanonicalAction::Reconfigure {
            revision: 1,
            target: RequestedConfig {
                model: Some("foreign".into()),
                ..Default::default()
            }
        }),
        DispatchOutcome::Rejected { .. }
    ));
    assert!(!fixture.root.join("config-writes").exists());
    assert!(matches!(
        handle.dispatch(&CanonicalAction::Reconfigure {
            revision: 1,
            target: target.clone()
        }),
        DispatchOutcome::Unknown { .. }
    ));
    match recv(&rx) {
        ProviderEvent::ConfigActivated {
            revision: 1,
            effective,
            ..
        } => {
            assert_eq!(effective.model, target.model);
            assert_eq!(effective.effort, target.effort);
        }
        event => panic!("unexpected {event:?}"),
    }
    assert!(matches!(
        handle.dispatch(&CanonicalAction::Reconfigure {
            revision: 1,
            target: target.clone()
        }),
        DispatchOutcome::Rejected { .. }
    ));
    let action = submit("wait for approval");
    handle.dispatch(&action);
    permission(&rx);
    assert!(matches!(
        handle.dispatch(&CanonicalAction::Reconfigure {
            revision: 2,
            target: target.clone()
        }),
        DispatchOutcome::Rejected { .. }
    ));
    let CanonicalAction::Submit { turn_id, .. } = action else {
        unreachable!()
    };
    handle.cancel(turn_id);
    assert!(matches!(recv(&rx), ProviderEvent::TurnCancelled { .. }));
    assert!(matches!(
        handle.dispatch(&CanonicalAction::Reconfigure {
            revision: 2,
            target: RequestedConfig {
                model: Some("ignored".into()),
                ..Default::default()
            }
        }),
        DispatchOutcome::Unknown { .. }
    ));
    assert!(matches!(recv(&rx), ProviderEvent::ProviderFailed { .. }));
    assert!(matches!(
        recv(&rx),
        ProviderEvent::Terminated {
            reason: TerminateReason::Interrupted { .. }
        }
    ));
    assert!(!handle.alive());
}

#[test]
fn admission_applies_explicit_configuration_before_session_live() {
    let fixture = Fixture::new();
    let (sink, rx) = sync_channel(16);
    let mut handle = DevinFactory::new(fixture.binary.clone())
        .spawn(SessionContext {
            session_id: Uuid::new_v4(),
            generation: 1,
            namespace: "devin-test".into(),
            adapter: "devin".into(),
            launch: json!({"cwd":fixture.root}),
            requested: RequestedConfig {
                model: Some("model-b".into()),
                effort: Some("high".into()),
                ..Default::default()
            },
            sink,
        })
        .unwrap();
    assert_eq!(recv(&rx), ProviderEvent::SessionLive);
    let ProviderEvent::ConfigActivated {
        revision: 0,
        effective,
        ..
    } = recv(&rx)
    else {
        panic!("missing activation")
    };
    assert_eq!(effective.model.as_deref(), Some("model-b"));
    assert_eq!(effective.effort.as_deref(), Some("high"));
    handle.terminate();
    recv(&rx);
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
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while host.status(&credential).unwrap().sessions[0].status
        == waku_runtime::contract::SessionStatus::Stopping
    {
        host.pump();
        assert!(std::time::Instant::now() < deadline);
        std::thread::yield_now();
    }
    assert_stopped_projection(&mut host, &credential, grant.epoch, cursor);
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
    assert_stopped_projection(&mut host, &credential, grant.epoch, cursor);
}

fn assert_stopped_projection(
    host: &mut RuntimeHost,
    credential: &str,
    epoch: u64,
    cursor: EventCursor,
) {
    // Subsequent pumps exercise sender disconnect after terminal ingestion.
    for _ in 0..3 {
        host.pump();
    }
    let events = host.events(credential, epoch, cursor).unwrap().events;
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e.event, ProviderEvent::Terminated { .. }))
            .count(),
        1
    );
    assert!(events.iter().any(|e| matches!(
        e.event,
        ProviderEvent::Terminated {
            reason: TerminateReason::Requested
        }
    )));
    let inventory = host.reconcile(credential, epoch).unwrap();
    assert!(!inventory.sessions[0].adapter_alive);
    assert_eq!(
        inventory.sessions[0].status,
        waku_runtime::contract::SessionStatus::Terminated
    );
}

#[test]
#[ignore = "requires installed authenticated Devin; disposable native configuration session"]
fn installed_devin_confirms_revisioned_model_and_effort() {
    let fixture = Fixture::new();
    let (sink, rx) = sync_channel(32);
    let mut handle = DevinFactory::new(PathBuf::from("/opt/homebrew/bin/devin"))
        .spawn(SessionContext {
            session_id: Uuid::new_v4(),
            generation: 1,
            namespace: "devin-config-live".into(),
            adapter: "devin".into(),
            launch: json!({"cwd":fixture.root}),
            requested: RequestedConfig {
                model: Some("swe-2-high".into()),
                effort: Some("high".into()),
                ..Default::default()
            },
            sink,
        })
        .unwrap();
    assert_eq!(recv(&rx), ProviderEvent::SessionLive);
    let ProviderEvent::ConfigActivated {
        effective: original,
        ..
    } = recv(&rx)
    else {
        panic!("no initial config")
    };
    for (revision, model, effort) in [(1, "swe-2-high", "max"), (2, "gpt-6-sol-medium", "medium")] {
        assert!(matches!(
            handle.dispatch(&CanonicalAction::Reconfigure {
                revision,
                target: RequestedConfig {
                    model: Some(model.into()),
                    effort: Some(effort.into()),
                    ..Default::default()
                }
            }),
            DispatchOutcome::Unknown { .. }
        ));
        let ProviderEvent::ConfigActivated {
            revision: actual,
            effective,
            ..
        } = recv(&rx)
        else {
            panic!("native configuration not confirmed")
        };
        assert_eq!(actual, revision);
        assert_eq!(effective.model.as_deref(), Some(model));
        assert_eq!(effective.effort.as_deref(), Some(effort));
        assert_eq!(effective.options, original.options);
        eprintln!(
            "Actual Devin config evidence: revision={revision}, model={model}, effort={effort}, identity={}",
            effective.options
        );
    }
    handle.terminate();
    assert!(matches!(
        recv(&rx),
        ProviderEvent::Terminated {
            reason: TerminateReason::Requested
        }
    ));
}

#[test]
#[ignore = "requires installed authenticated Devin; billable disposable approval and cancel turns"]
fn installed_devin_approval_and_cancel_preserve_original_responders() {
    let fixture = Fixture::new();
    let (sink, rx) = sync_channel(128);
    let mut handle = DevinFactory::new(PathBuf::from("/opt/homebrew/bin/devin"))
        .spawn(SessionContext {
            session_id: Uuid::new_v4(),
            generation: 1,
            namespace: "devin-approval-live".into(),
            adapter: "devin".into(),
            launch: json!({"cwd":fixture.root}),
            requested: RequestedConfig::default(),
            sink,
        })
        .unwrap();
    assert_eq!(recv(&rx), ProviderEvent::SessionLive);
    recv(&rx);
    for (marker, cancel) in [("approved-marker", false), ("cancelled-marker", true)] {
        let action = submit(&format!(
            "Execute exactly this shell command using your shell tool: python3 -c \"from pathlib import Path; Path('{marker}').write_text('confirmed')\". Do not use an edit tool or any other command. Then reply DONE."
        ));
        let CanonicalAction::Submit { turn_id, .. } = action else {
            unreachable!()
        };
        handle.dispatch(&action);
        let deadline = std::time::Instant::now() + Duration::from_secs(120);
        let (id, subject) = loop {
            let event = rx.recv_timeout(Duration::from_secs(120)).unwrap();
            match event {
                ProviderEvent::RequestOpened {
                    request_id,
                    class: RequestClass::Approval,
                    subject,
                    ..
                } => break (request_id, subject),
                ProviderEvent::TurnCompleted { .. } => {
                    panic!("native provider completed without approval; gate not established")
                }
                ProviderEvent::ProviderFailed { detail } => panic!("{detail}"),
                _ => {}
            }
            assert!(std::time::Instant::now() < deadline);
        };
        assert!(
            !fixture.root.join(marker).exists(),
            "execution preceded approval"
        );
        // This is a measured observation window, not an upper bound on native deadlines.
        let observed = std::time::Instant::now();
        while observed.elapsed() < Duration::from_secs(2) {
            match rx.recv_timeout(Duration::from_secs(2).saturating_sub(observed.elapsed())) {
                Ok(ProviderEvent::TurnDelta { .. }) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => break,
                other => panic!("approval did not remain pending: {other:?}"),
            }
            assert!(!fixture.root.join(marker).exists());
        }
        eprintln!(
            "Actual Devin original approval retained for {} ms; cancel={cancel}",
            observed.elapsed().as_millis()
        );
        if cancel {
            assert!(matches!(
                handle.cancel(turn_id),
                CancelOutcome::Unknown { .. }
            ));
        } else {
            let option = subject["options"]
                .as_array()
                .unwrap()
                .iter()
                .find(|o| o["kind"] == "allow_once")
                .unwrap()["optionId"]
                .as_str()
                .unwrap();
            assert!(matches!(
                handle.dispatch(&CanonicalAction::ResolveRequest {
                    request_id: id.clone(),
                    resolution: json!({"optionId":option})
                }),
                DispatchOutcome::Unknown { .. }
            ));
        }
        loop {
            match rx.recv_timeout(Duration::from_secs(120)).unwrap() {
                ProviderEvent::TurnCompleted {
                    turn_id: actual, ..
                } if !cancel => {
                    assert_eq!(actual, turn_id);
                    break;
                }
                ProviderEvent::TurnCancelled { turn_id: actual } if cancel => {
                    assert_eq!(actual, turn_id);
                    break;
                }
                ProviderEvent::ProviderFailed { detail } => panic!("{detail}"),
                ProviderEvent::RequestAnswered { .. } => {
                    panic!("fabricated consumption acknowledgement")
                }
                _ => {}
            }
            assert!(std::time::Instant::now() < deadline);
        }
        assert_eq!(fixture.root.join(marker).exists(), !cancel);
        assert!(matches!(
            handle.dispatch(&CanonicalAction::ResolveRequest {
                request_id: id,
                resolution: json!({"optionId":"allow"})
            }),
            DispatchOutcome::Rejected { .. }
        ));
    }
    handle.terminate();
    assert!(matches!(
        recv(&rx),
        ProviderEvent::Terminated {
            reason: TerminateReason::Requested
        }
    ));
}

#[test]
fn recovered_devin_owner_blocks_reconciliation_and_new_admission_without_pid_signalling() {
    let fixture = Fixture::new();
    let root = fixture.root.join("recovery-host");
    let factory = Arc::new(DevinFactory::new(fixture.binary.clone()));
    let mut host = RuntimeHost::open(
        &root,
        HostOptions::new("devin-recovery"),
        factory.clone(),
        Box::new(ManualClock::new(1_000_000)),
    )
    .unwrap();
    let credential = host.credentials().controller.clone();
    let grant = host
        .acquire_control(&credential, &ClientHello::current())
        .unwrap();
    let original = Uuid::new_v4();
    host.execute(
        &credential,
        ActionEnvelope {
            namespace: "devin-recovery".into(),
            runtime_instance: host.instance(),
            epoch: grant.epoch,
            retry_window_id: grant.retry_window_id,
            command_id: Uuid::new_v4(),
            session: None,
            action: CanonicalAction::AdmitSession {
                session_id: original,
                adapter: "devin".into(),
                launch: json!({"cwd":fixture.root}),
                requested: RequestedConfig::default(),
            },
        },
    )
    .unwrap();
    host.pump();
    drop(host); // no durable drain: physical cleanup cannot be inferred from a PID
    let mut host = RuntimeHost::open(
        &root,
        HostOptions::new("devin-recovery"),
        factory.clone(),
        Box::new(ManualClock::new(1_000_000)),
    )
    .unwrap();
    let grant = host
        .acquire_control(&credential, &ClientHello::current())
        .unwrap();
    assert_eq!(
        host.status(&credential).unwrap().unfenced_survivors,
        vec![SessionRef {
            session_id: original,
            generation: 1
        }]
    );
    assert_eq!(
        host.reconcile(&credential, grant.epoch),
        Err(waku_runtime::contract::HostError::NotReconciled)
    );
    assert!(matches!(
        host.execute(
            &credential,
            ActionEnvelope {
                namespace: "devin-recovery".into(),
                runtime_instance: host.instance(),
                epoch: grant.epoch,
                retry_window_id: grant.retry_window_id,
                command_id: Uuid::new_v4(),
                session: None,
                action: CanonicalAction::AdmitSession {
                    session_id: Uuid::new_v4(),
                    adapter: "devin".into(),
                    launch: json!({"cwd":fixture.root}),
                    requested: RequestedConfig::default()
                }
            }
        ),
        Ok(waku_runtime::contract::CommandReceipt {
            disposition: CommandDisposition::Rejected { .. },
            ..
        })
    ));
    let mut unrelated = Helper(
        std::process::Command::new("/bin/sleep")
            .arg("60")
            .spawn()
            .unwrap(),
    );
    assert!(matches!(
        factory.fence_recovered(&RecoveredOwner {
            namespace: "devin-recovery".into(),
            session: SessionRef {
                session_id: original,
                generation: 1
            },
            adapter: "devin".into(),
            launch: json!({"cwd":fixture.root}),
            effective: Some(waku_runtime::contract::EffectiveConfig {
                revision: 0,
                boundary: ActivationBoundary::Immediate,
                config: RequestedConfig {
                    options: json!({"ownedPid":unrelated.0.id()}),
                    ..Default::default()
                }
            })
        }),
        Termination::Unconfirmed { .. }
    ));
    assert!(unrelated.0.try_wait().unwrap().is_none());
}

#[test]
fn unanswered_configuration_expires_without_activation_or_prompt_dispatch() {
    let fixture = Fixture::new();
    let (mut handle, rx) = fixture.spawn(16);
    let start = std::time::Instant::now();
    assert!(matches!(
        handle.dispatch(&CanonicalAction::Reconfigure {
            revision: 1,
            target: RequestedConfig {
                model: Some("no-response".into()),
                ..Default::default()
            }
        }),
        DispatchOutcome::Unknown { .. }
    ));
    assert!(matches!(
        handle.dispatch(&submit("must not run")),
        DispatchOutcome::Rejected { .. }
    ));
    assert!(matches!(
        rx.recv_timeout(Duration::from_secs(15)).unwrap(),
        ProviderEvent::ProviderFailed { .. }
    ));
    assert!(start.elapsed() < Duration::from_secs(15));
    assert!(matches!(
        recv(&rx),
        ProviderEvent::Terminated {
            reason: TerminateReason::Interrupted { .. }
        }
    ));
    assert!(!fixture.root.join("prompts").exists());
    assert!(!handle.alive());
}

#[test]
#[cfg(unix)]
fn devin_child_env_carries_only_the_provisioned_ingress_pairs() {
    use std::time::Instant;
    let root = std::env::temp_dir().join(format!("runtime-devin-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    // A fixture variant that records its environment instead of asserting
    // the credential scrub left nothing behind.
    let binary = root.join("devin-env");
    let fixture = include_str!("fixture.py").replace(
        "assert not any(key.startswith((\"GODDARD_\", \"WAKU_\")) for key in os.environ)",
        "Path(\"agent-env.json\").open(\"w\").write(json.dumps({\"goddard\": sorted(k for k in os.environ if k.startswith(\"GODDARD_\")), \"path\": os.environ.get(\"PATH\", \"\")}))",
    );
    assert_ne!(fixture, include_str!("fixture.py"), "replacement applied");
    std::fs::write(&binary, fixture).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();

    let cli_dir = root.join("pinned-helpers");
    let (sink, _rx) = sync_channel(16);
    let handle = DevinFactory::new(binary)
        .spawn(SessionContext {
            session_id: Uuid::new_v4(),
            generation: 1,
            namespace: "devin-test".into(),
            adapter: "devin".into(),
            launch: json!({
                "cwd": root,
                "agent": {
                    "env": {
                        "GODDARD_RUNTIME_INGRESS": "/private/ingress.json",
                        "GODDARD_TASK_ID": "task-1",
                    },
                    "cliDir": cli_dir,
                }
            }),
            requested: RequestedConfig::default(),
            sink,
        })
        .unwrap();

    let env_path = root.join("agent-env.json");
    let deadline = Instant::now() + Duration::from_secs(10);
    while !env_path.exists() {
        assert!(
            Instant::now() < deadline,
            "the child never recorded its env"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    let dump: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&env_path).unwrap()).unwrap();
    let goddard = dump["goddard"].as_array().unwrap();
    assert!(goddard.contains(&json!("GODDARD_RUNTIME_INGRESS")));
    assert!(goddard.contains(&json!("GODDARD_TASK_ID")));
    assert!(
        !goddard.contains(&json!("GODDARD_AGENT_TOKEN"))
            && !goddard.contains(&json!("GODDARD_DAEMON_ADDRESS")),
        "the daemon credential never reaches the provider: {goddard:?}"
    );
    assert!(
        dump["path"]
            .as_str()
            .unwrap()
            .starts_with(&format!("{}:", cli_dir.display())),
        "the pinned helper directory leads PATH: {}",
        dump["path"]
    );
    drop(handle);
    let _ = std::fs::remove_dir_all(&root);
}
