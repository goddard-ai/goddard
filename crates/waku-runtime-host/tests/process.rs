#![cfg(unix)]
use std::{
    fs,
    path::PathBuf,
    process::Command,
    time::{Duration, Instant},
};
use waku_runtime::{ipc::Endpoint, *};
struct Host {
    root: PathBuf,
    endpoint: Endpoint,
    token: String,
}
impl Host {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "runtime-process-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let endpoint = launch(&root);
        let identity: serde_json::Value =
            serde_json::from_slice(&fs::read(root.join("identity.json")).unwrap()).unwrap();
        Self {
            root,
            endpoint,
            token: identity["controller"].as_str().unwrap().into(),
        }
    }
    fn client(&self) -> RuntimeClient {
        RuntimeClient::controller(&self.root, &self.token, &ClientHello::current())
            .unwrap()
            .0
    }
}
impl Drop for Host {
    fn drop(&mut self) {
        if let Ok((mut client, _)) =
            RuntimeClient::controller(&self.root, &self.token, &ClientHello::current())
            && let Ok(grant) = client.acquire_control(&ClientHello::current())
        {
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                if matches!(
                    client.drain(grant.epoch, DrainPolicy::Forced),
                    Ok(DrainReport::Drained { .. })
                ) || Instant::now() >= deadline
                {
                    break;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        let deadline = Instant::now() + Duration::from_secs(6);
        while Endpoint::path(&self.root).exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        if Endpoint::path(&self.root).exists() {
            unsafe {
                libc::kill(self.endpoint.pid as i32, libc::SIGKILL);
            }
        }
        let _ = fs::remove_dir_all(&self.root);
    }
}
fn launch(root: &PathBuf) -> Endpoint {
    let output = Command::new(env!("CARGO_BIN_EXE_waku-runtime-host"))
        .args(["start", "--namespace", "process-test", "--root"])
        .arg(root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}
#[test]
fn pinned_owner_survives_launcher_and_25_controller_replacements() {
    let host = Host::new();
    assert!(
        host.root
            .join("artifacts")
            .read_dir()
            .unwrap()
            .next()
            .is_some()
    );
    assert_eq!(launch(&host.root), host.endpoint);
    let mut old = host.client();
    let old_epoch = old.acquire_control(&ClientHello::current()).unwrap().epoch;
    for _ in 0..25 {
        assert!(
            host.client()
                .acquire_control(&ClientHello::current())
                .unwrap()
                .epoch
                > old_epoch
        );
    }
    assert!(matches!(
        old.reconcile(old_epoch),
        Err(ClientError::Host(HostError::StaleEpoch { .. }))
    ));
    assert!(matches!(
        old.drain(old_epoch, DrainPolicy::Forced),
        Err(ClientError::Host(HostError::StaleEpoch { .. }))
    ));
    assert_eq!(launch(&host.root), host.endpoint);
}
#[test]
fn stale_discovery_and_incompatible_controller_preserve_owner() {
    let host = Host::new();
    assert!(matches!(
        RuntimeClient::controller(&host.root, "wrong", &ClientHello::current()),
        Err(ClientError::Host(HostError::Unauthorized))
    ));
    let mut required = ClientHello::current();
    required.capabilities.push("unsupported-required".into());
    assert!(matches!(
        RuntimeClient::controller(&host.root, &host.token, &required),
        Err(ClientError::Host(HostError::Incompatible(_)))
    ));
    let original = fs::read(Endpoint::path(&host.root)).unwrap();
    let mut stale: serde_json::Value = serde_json::from_slice(&original).unwrap();
    stale["instance"] = serde_json::json!("00000000-0000-0000-0000-000000000000");
    fs::write(
        Endpoint::path(&host.root),
        serde_json::to_vec(&stale).unwrap(),
    )
    .unwrap();
    let result = RuntimeClient::controller(&host.root, &host.token, &ClientHello::current());
    fs::write(Endpoint::path(&host.root), original).unwrap();
    assert!(matches!(
        result,
        Err(ClientError::Host(HostError::Unauthorized))
    ));
    assert_eq!(launch(&host.root), host.endpoint);
}

#[test]
fn saturated_normal_connections_leave_emergency_status_available() {
    let host = Host::new();
    let identity: serde_json::Value =
        serde_json::from_slice(&fs::read(host.root.join("identity.json")).unwrap()).unwrap();
    let peers: Vec<_> = (0..8)
        .map(|_| std::os::unix::net::UnixStream::connect(&host.endpoint.socket).unwrap())
        .collect();
    std::thread::sleep(Duration::from_millis(50));
    let (_, status) =
        RuntimeClient::emergency(&host.root, identity["emergency"].as_str().unwrap()).unwrap();
    assert_eq!(status.runtime_instance, host.endpoint.instance);
    drop(peers);
}

fn envelope(
    host: &Host,
    grant: &ControlGrant,
    session: Option<SessionRef>,
    id: uuid::Uuid,
    action: CanonicalAction,
) -> ActionEnvelope {
    ActionEnvelope {
        namespace: host.endpoint.namespace.clone(),
        runtime_instance: host.endpoint.instance,
        epoch: grant.epoch,
        retry_window_id: grant.retry_window_id,
        session,
        command_id: id,
        action,
    }
}
fn wait_events(
    client: &mut RuntimeClient,
    grant: &ControlGrant,
    session: SessionRef,
    condition: impl Fn(&[EventEnvelope]) -> bool,
) -> Vec<EventEnvelope> {
    let deadline = Instant::now() + Duration::from_secs(120);
    loop {
        let events = client
            .events(
                grant.epoch,
                EventCursor {
                    session_id: session.session_id,
                    generation: session.generation,
                    applied: 0,
                },
                20,
            )
            .unwrap()
            .events;
        if condition(&events) {
            return events;
        }
        assert!(
            Instant::now() < deadline,
            "provider barrier timed out ({} events)",
            events.len()
        );
    }
}
fn admit_codex(host: &Host, client: &mut RuntimeClient, grant: &ControlGrant) -> SessionRef {
    use std::os::unix::fs::PermissionsExt;
    let binary = host.root.join("codex-fixture");
    fs::write(
        &binary,
        include_str!("../../waku-runtime-codex/tests/app_server_fixture.py"),
    )
    .unwrap();
    fs::set_permissions(&binary, fs::Permissions::from_mode(0o700)).unwrap();
    let session = SessionRef {
        session_id: uuid::Uuid::new_v4(),
        generation: 1,
    };
    let result = client.execute(envelope(host, grant, None, uuid::Uuid::new_v4(), CanonicalAction::AdmitSession {
        session_id: session.session_id, adapter: "codex".into(),
        launch: serde_json::json!({"binary":binary,"cwd":host.root,"approvalPolicy":"on-request","sandbox":"read-only"}),
        requested: RequestedConfig::default(),
    })).unwrap();
    assert!(
        matches!(result.disposition, CommandDisposition::Terminal { .. }),
        "{result:?}"
    );
    session
}
#[test]
fn codex_original_callbacks_and_command_identity_survive_executable_controller_changes() {
    let host = Host::new();
    let mut client = host.client();
    let mut grant = client.acquire_control(&ClientHello::current()).unwrap();
    let session = admit_codex(&host, &mut client, &grant);
    let turn = uuid::Uuid::new_v4();
    let id = uuid::Uuid::new_v4();
    let submit = CanonicalAction::Submit {
        delivery_id: uuid::Uuid::new_v4(),
        turn_id: turn,
        input: serde_json::json!("callbacks"),
    };
    let first = client
        .execute(envelope(&host, &grant, Some(session), id, submit.clone()))
        .unwrap();
    assert_eq!(first.disposition, CommandDisposition::ProviderAccepted);
    let opened = wait_events(&mut client, &grant, session, |e| {
        e.iter().any(|e| {
            matches!(
                e.event,
                ProviderEvent::RequestOpened {
                    class: RequestClass::Tool,
                    ..
                }
            )
        })
    });
    assert!(opened.iter().any(|e| matches!(&e.event, ProviderEvent::ConfigActivated { effective, .. } if effective.options["codexThreadId"]=="fixture-thread")));
    let inherited: Vec<String> =
        serde_json::from_slice(&fs::read(host.root.join("provider-environment.json")).unwrap())
            .unwrap();
    assert!(
        inherited.is_empty(),
        "ambient Goddard capabilities inherited"
    );
    let old_epoch = grant.epoch;
    for _ in 0..25 {
        drop(client);
        client = host.client();
        grant = client.acquire_control(&ClientHello::current()).unwrap();
        let inventory = client.reconcile(grant.epoch).unwrap();
        assert_eq!(inventory.sessions.len(), 1);
        assert_eq!(
            client
                .execute(envelope(&host, &grant, Some(session), id, submit.clone()))
                .unwrap()
                .disposition,
            first.disposition
        );
    }
    assert!(grant.epoch > old_epoch);
    for (class, response) in [
        (
            RequestClass::Tool,
            serde_json::json!({"success":true,"contentItems":[{"type":"inputText","text":"one-result"}]}),
        ),
        (
            RequestClass::Approval,
            serde_json::json!({"decision":"decline"}),
        ),
        (RequestClass::Input, serde_json::json!({"answers":{}})),
    ] {
        let events = wait_events(&mut client, &grant, session, |e| {
            e.iter().any(
                |e| matches!(&e.event, ProviderEvent::RequestOpened { class:c, .. } if *c==class),
            )
        });
        let request = events
            .iter()
            .find_map(|e| match &e.event {
                ProviderEvent::RequestOpened {
                    class: c,
                    request_id,
                    ..
                } if *c == class => Some(request_id.clone()),
                _ => None,
            })
            .unwrap();
        let command = uuid::Uuid::new_v4();
        let action = CanonicalAction::ResolveRequest {
            request_id: request,
            resolution: response,
        };
        let receipt = client
            .execute(envelope(
                &host,
                &grant,
                Some(session),
                command,
                action.clone(),
            ))
            .unwrap();
        drop(client);
        client = host.client();
        grant = client.acquire_control(&ClientHello::current()).unwrap();
        client.reconcile(grant.epoch).unwrap();
        let duplicate = client
            .execute(envelope(&host, &grant, Some(session), command, action))
            .unwrap();
        // A synchronously captured native result may reconcile uncertainty.
        assert!(
            duplicate.disposition == receipt.disposition
                || matches!(
                    duplicate.disposition,
                    CommandDisposition::Terminal {
                        outcome: TerminalOutcome::Completed,
                        event_sequence: Some(_)
                    }
                )
        );
    }
    let events = wait_events(&mut client, &grant, session, |e| {
        e.iter()
            .any(|e| matches!(e.event, ProviderEvent::TurnCompleted { .. }))
    });
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
            .all(|pair| pair[0].sequence + 1 == pair[1].sequence)
    );
    let messages: Vec<serde_json::Value> = fs::read_to_string(host.root.join("received.jsonl"))
        .unwrap()
        .lines()
        .map(|s| serde_json::from_str(s).unwrap())
        .collect();
    assert_eq!(
        messages
            .iter()
            .filter(|m| m["method"] == "turn/start")
            .count(),
        1
    );
    assert!(!messages.iter().any(|m| m["method"] == "thread/resume"));
    for native in [
        serde_json::json!(11),
        serde_json::json!("11"),
        serde_json::json!("input"),
    ] {
        assert_eq!(
            messages
                .iter()
                .filter(|m| m["method"].is_null() && m["id"] == native)
                .count(),
            1
        );
    }
    assert_eq!(
        fs::read_to_string(host.root.join("provider-starts.txt"))
            .unwrap()
            .lines()
            .count(),
        1
    );
}

#[test]
fn runtime_crash_fences_codex_group_and_preserves_uncertain_work_without_resume() {
    let mut host = Host::new();
    let mut client = host.client();
    let grant = client.acquire_control(&ClientHello::current()).unwrap();
    let session = admit_codex(&host, &mut client, &grant);
    client
        .execute(envelope(
            &host,
            &grant,
            Some(session),
            uuid::Uuid::new_v4(),
            CanonicalAction::Submit {
                delivery_id: uuid::Uuid::new_v4(),
                turn_id: uuid::Uuid::new_v4(),
                input: serde_json::json!("callbacks"),
            },
        ))
        .unwrap();
    wait_events(&mut client, &grant, session, |events| {
        events.iter().any(|e| {
            matches!(
                e.event,
                ProviderEvent::RequestOpened {
                    class: RequestClass::Tool,
                    ..
                }
            )
        })
    });
    unsafe {
        assert_eq!(libc::kill(host.endpoint.pid as i32, libc::SIGKILL), 0);
    }
    drop(client);
    let deadline = Instant::now() + Duration::from_secs(5);
    while launch_if_ready(&host.root).is_none() {
        assert!(Instant::now() < deadline, "replacement launch timed out");
        std::thread::sleep(Duration::from_millis(20));
    }
    host.endpoint = Endpoint::load(&host.root).unwrap();
    let mut client = host.client();
    let grant = client.acquire_control(&ClientHello::current()).unwrap();
    let inventory = client.reconcile(grant.epoch).unwrap();
    assert!(matches!(
        inventory.sessions[0].status,
        SessionStatus::Interrupted
    ));
    assert!(client.status().unwrap().unfenced_survivors.is_empty());
    assert_eq!(
        fs::read_to_string(host.root.join("provider-starts.txt"))
            .unwrap()
            .lines()
            .count(),
        1
    );
    let messages = fs::read_to_string(host.root.join("received.jsonl")).unwrap();
    assert!(!messages.contains("thread/resume"));
    assert_eq!(
        messages
            .lines()
            .filter(|line| line.contains("turn/start"))
            .count(),
        1
    );
}
fn launch_if_ready(root: &PathBuf) -> Option<Endpoint> {
    let output = Command::new(env!("CARGO_BIN_EXE_waku-runtime-host"))
        .args(["start", "--namespace", "process-test", "--root"])
        .arg(root)
        .output()
        .unwrap();
    output
        .status
        .success()
        .then(|| serde_json::from_slice(&output.stdout).unwrap())
}

#[test]
#[ignore = "requires CODEX_BINARY and authenticated actual Codex; disposable read-only native session"]
fn actual_codex_callback_through_independent_executable() {
    let host = Host::new();
    let mut client = host.client();
    let mut grant = client.acquire_control(&ClientHello::current()).unwrap();
    let session = SessionRef {
        session_id: uuid::Uuid::new_v4(),
        generation: 1,
    };
    let binary = std::env::var("CODEX_BINARY").expect("set CODEX_BINARY");
    let receipt = client.execute(envelope(&host,&grant,None,uuid::Uuid::new_v4(),CanonicalAction::AdmitSession {session_id:session.session_id,adapter:"codex".into(),launch:serde_json::json!({"binary":binary,"cwd":host.root,"approvalPolicy":"on-request","sandbox":"read-only","dynamicTools":[{"name":"runtime_probe","description":"Return the runtime probe token. Call once when instructed.","inputSchema":{"type":"object","properties":{},"additionalProperties":false}}]}),requested:RequestedConfig::default()})).unwrap();
    assert!(
        matches!(receipt.disposition, CommandDisposition::Terminal { .. }),
        "actual admission: {receipt:?}"
    );
    client.execute(envelope(&host,&grant,Some(session),uuid::Uuid::new_v4(),CanonicalAction::Submit{delivery_id:uuid::Uuid::new_v4(),turn_id:uuid::Uuid::new_v4(),input:serde_json::json!("Call runtime_probe exactly once, then reply with the token it returns. Do not use other tools.")})).unwrap();
    let events = wait_events(&mut client, &grant, session, |events| {
        events.iter().any(|e| {
            matches!(
                e.event,
                ProviderEvent::RequestOpened {
                    class: RequestClass::Tool,
                    ..
                }
            )
        })
    });
    let request = events
        .iter()
        .find_map(|e| match &e.event {
            ProviderEvent::RequestOpened {
                request_id,
                class: RequestClass::Tool,
                ..
            } => Some(request_id.clone()),
            _ => None,
        })
        .unwrap();
    for _ in 0..25 {
        drop(client);
        client = host.client();
        grant = client.acquire_control(&ClientHello::current()).unwrap();
        client.reconcile(grant.epoch).unwrap();
    }
    client.execute(envelope(&host,&grant,Some(session),uuid::Uuid::new_v4(),CanonicalAction::ResolveRequest{request_id:request.clone(),resolution:serde_json::json!({"success":true,"contentItems":[{"type":"inputText","text":"stable-runtime-native-probe"}]})})).unwrap();
    drop(client);
    std::thread::sleep(Duration::from_millis(100));
    client = host.client();
    grant = client.acquire_control(&ClientHello::current()).unwrap();
    client.reconcile(grant.epoch).unwrap();
    let events = wait_events(&mut client, &grant, session, |events| {
        events
            .iter()
            .any(|e| matches!(e.event, ProviderEvent::TurnCompleted { .. }))
    });
    assert!(events.iter().any(
        |e| matches!(&e.event,ProviderEvent::RequestAnswered{request_id} if request_id==&request)
    ));
    assert!(events.iter().any(|e|matches!(&e.event,ProviderEvent::TurnDelta{delta,..} if delta.to_string().contains("stable-runtime-native-probe"))));
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e.event, ProviderEvent::TurnCompleted { .. }))
            .count(),
        1
    );
    let thread_id = events
        .iter()
        .find_map(|e| match &e.event {
            ProviderEvent::ConfigActivated { effective, .. } => {
                effective.options["codexThreadId"].as_str()
            }
            _ => None,
        })
        .unwrap();
    let native_turn = events
        .iter()
        .find_map(|e| match &e.event {
            ProviderEvent::TurnDelta { delta, .. } if delta["method"] == "turn/started" => delta
                .pointer("/params/turn/id")
                .and_then(serde_json::Value::as_str),
            _ => None,
        })
        .unwrap();
    eprintln!(
        "actual Codex original transport qualified: runtime={} thread={} turn={} controller_transfers=25 terminal_count=1",
        host.endpoint.instance, thread_id, native_turn
    );
    let model = events
        .iter()
        .find_map(|e| match &e.event {
            ProviderEvent::ConfigActivated { effective, .. } => effective.model.clone(),
            _ => None,
        })
        .expect("actual native model");
    let configured = client
        .execute(envelope(
            &host,
            &grant,
            Some(session),
            uuid::Uuid::new_v4(),
            CanonicalAction::Reconfigure {
                revision: 1,
                target: RequestedConfig {
                    provider: None,
                    model: Some(model.clone()),
                    effort: Some("low".into()),
                    options: serde_json::Value::Null,
                },
            },
        ))
        .unwrap();
    assert_eq!(configured.disposition, CommandDisposition::ProviderAccepted);
    let cancelled = uuid::Uuid::new_v4();
    client
        .execute(envelope(
            &host,
            &grant,
            Some(session),
            uuid::Uuid::new_v4(),
            CanonicalAction::Submit {
                delivery_id: uuid::Uuid::new_v4(),
                turn_id: cancelled,
                input: serde_json::json!("Call runtime_probe once and wait for its result."),
            },
        ))
        .unwrap();
    wait_events(&mut client, &grant, session, |events| {
        events.iter().any(
            |e| matches!(e.event,ProviderEvent::TurnStarted {turn_id,..} if turn_id==cancelled),
        )
    });
    wait_events(&mut client, &grant, session, |events| {
        events.iter().any(|e|matches!(&e.event,ProviderEvent::ConfigActivated { revision:1,effective,.. } if effective.model.as_deref()==Some(&model) && effective.effort.as_deref()==Some("low")))
    });
    client
        .execute(envelope(
            &host,
            &grant,
            Some(session),
            uuid::Uuid::new_v4(),
            CanonicalAction::CancelTurn { turn_id: cancelled },
        ))
        .unwrap();
    wait_events(&mut client, &grant, session, |events| {
        events
            .iter()
            .any(|e| matches!(e.event,ProviderEvent::TurnCancelled {turn_id} if turn_id==cancelled))
    });
    eprintln!(
        "actual Codex native model/effort activation and cancel qualified for runtime turn={cancelled}"
    );
    client
        .execute(envelope(
            &host,
            &grant,
            Some(session),
            uuid::Uuid::new_v4(),
            CanonicalAction::PermitDispatch,
        ))
        .unwrap();
    let floor = client
        .events(
            grant.epoch,
            EventCursor {
                session_id: session.session_id,
                generation: 1,
                applied: 0,
            },
            0,
        )
        .unwrap()
        .high_water;
    client
        .execute(envelope(
            &host,
            &grant,
            Some(session),
            uuid::Uuid::new_v4(),
            CanonicalAction::Submit {
                delivery_id: uuid::Uuid::new_v4(),
                turn_id: uuid::Uuid::new_v4(),
                input: serde_json::json!(
                    "Call runtime_probe exactly once and wait for its result."
                ),
            },
        ))
        .unwrap();
    wait_events(&mut client, &grant, session, |events| {
        events.iter().any(|e| {
            e.sequence > floor
                && matches!(
                    e.event,
                    ProviderEvent::RequestOpened {
                        class: RequestClass::Tool,
                        ..
                    }
                )
        })
    });
    drop(client);
    let identity: serde_json::Value =
        serde_json::from_slice(&fs::read(host.root.join("identity.json")).unwrap()).unwrap();
    let (mut emergency, _) =
        RuntimeClient::emergency(&host.root, identity["emergency"].as_str().unwrap()).unwrap();
    emergency.emergency_stop(session.session_id).unwrap();
    client = host.client();
    grant = client.acquire_control(&ClientHello::current()).unwrap();
    wait_events(&mut client, &grant, session, |events| {
        events.iter().any(|e| {
            matches!(
                e.event,
                ProviderEvent::Terminated {
                    reason: TerminateReason::Requested
                }
            )
        })
    });
    let inventory = client.reconcile(grant.epoch).unwrap();
    assert_eq!(inventory.sessions[0].status, SessionStatus::Terminated);
    let late = client
        .execute(envelope(
            &host,
            &grant,
            Some(session),
            uuid::Uuid::new_v4(),
            CanonicalAction::Submit {
                delivery_id: uuid::Uuid::new_v4(),
                turn_id: uuid::Uuid::new_v4(),
                input: serde_json::json!("must not dispatch"),
            },
        ))
        .unwrap();
    assert!(matches!(
        late.disposition,
        CommandDisposition::Rejected {
            code: RejectCode::Fenced,
            ..
        }
    ));
    eprintln!("actual Codex detached pending tool emergency-stop and late-submit fence qualified");
}
