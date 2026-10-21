#![cfg(unix)]
use std::{
    fs,
    path::PathBuf,
    process::{Command, Stdio},
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
    client = host.client();
    grant = client.acquire_control(&ClientHello::current()).unwrap();
    // The barrier is the reconciled inventory, not elapsed time: the
    // answered request's consumption either reconciled or remains pending.
    assert!(!client.reconcile(grant.epoch).unwrap().sessions.is_empty());
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

/// Actual-provider qualification for the scoped agent surface: the real
/// `goddard-agent` binary rides runtime ingress under the session's own
/// grant — inside and outside the provider's process tree — while native
/// tool and approval replies cross 25 controller replacements on the
/// original transport. Disposable namespace only; no product daemon, app,
/// watcher, or existing provider is touched.
#[test]
#[ignore = "requires CODEX_BINARY + GODDARD_AGENT_BINARY and authenticated actual Codex; disposable session"]
fn actual_codex_scoped_cli_ingress_and_native_request_boundaries() {
    use std::os::unix::fs::PermissionsExt;
    let host = Host::new();
    let mut client = host.client();
    let mut grant = client.acquire_control(&ClientHello::current()).unwrap();
    let session = SessionRef {
        session_id: uuid::Uuid::new_v4(),
        generation: 1,
    };
    let task = uuid::Uuid::new_v4();
    // Provision the agent surface the way the daemon does: a helper
    // directory for the pinned CLI plus provisioned env pairs; the grant
    // itself only ever travels through a private descriptor file.
    let cli_dir = host.root.join("cli");
    let agent_dir = host.root.join("agent").join(task.to_string());
    let workspace = host.root.join("workspace");
    fs::create_dir_all(&cli_dir).unwrap();
    fs::create_dir_all(&agent_dir).unwrap();
    fs::create_dir_all(&workspace).unwrap();
    let helper = cli_dir.join("goddard-agent");
    fs::copy(
        std::env::var("GODDARD_AGENT_BINARY").expect("set GODDARD_AGENT_BINARY"),
        &helper,
    )
    .unwrap();
    fs::set_permissions(&helper, fs::Permissions::from_mode(0o500)).unwrap();
    let descriptor = agent_dir.join("ingress.json");
    let binary = std::env::var("CODEX_BINARY").expect("set CODEX_BINARY");
    let version = Command::new(&binary)
        .arg("--version")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        .unwrap_or_else(|_| "unknown".into());
    let admit = client
        .execute(envelope(
            &host,
            &grant,
            None,
            uuid::Uuid::new_v4(),
            CanonicalAction::AdmitSession {
                session_id: session.session_id,
                adapter: "codex".into(),
                launch: serde_json::json!({
                    "binary":binary,"cwd":workspace,"approvalPolicy":"on-request","sandbox":"read-only",
                    "developerInstructions":"You are inside a disposable stable-runtime qualification session. Follow each instruction literally, use the fewest possible steps, and do nothing else.",
                    "dynamicTools":[{"name":"runtime_probe","description":"Return the runtime probe token. Call once when instructed.","inputSchema":{"type":"object","properties":{},"additionalProperties":false}}],
                    "agent":{"env":{"GODDARD_RUNTIME_INGRESS":descriptor,"GODDARD_TASK_ID":task},"cliDir":cli_dir},
                }),
                requested: RequestedConfig::default(),
            },
        ))
        .unwrap();
    assert!(
        matches!(admit.disposition, CommandDisposition::Terminal { .. }),
        "actual admission: {admit:?}"
    );
    let agent_grant = client.issue_grant(grant.epoch, session, 600_000).unwrap();
    fs::write(
        &descriptor,
        serde_json::to_vec(&IngressDescriptor {
            version: INGRESS_DESCRIPTOR_VERSION,
            root: host.root.clone(),
            task_id: task,
            grant: agent_grant.clone(),
        })
        .unwrap(),
    )
    .unwrap();
    fs::set_permissions(&descriptor, fs::Permissions::from_mode(0o600)).unwrap();
    client
        .register_claim(
            grant.epoch,
            ResourceClaim {
                claim_id: uuid::Uuid::new_v4(),
                session,
                resources: vec!["agent-ingress".into(), "cli-helper".into()],
                detail: serde_json::json!({
                    "descriptor":descriptor,
                    "helperDir":cli_dir,
                    "grantId":agent_grant.grant_id,
                    "reservations":[],
                }),
            },
        )
        .unwrap();
    // The provider group owner is a stable, live pid that must not change
    // across controller replacement.
    let events = wait_events(&mut client, &grant, session, |e| {
        e.iter()
            .any(|e| matches!(e.event, ProviderEvent::ConfigActivated { .. }))
    });
    let (owner_pid, thread_id) = events
        .iter()
        .find_map(|e| match &e.event {
            ProviderEvent::ConfigActivated { effective, .. } => Some((
                effective.options["ownerGroupPid"].as_u64().unwrap() as i32,
                effective.options["codexThreadId"].as_str().unwrap().to_owned(),
            )),
            _ => None,
        })
        .unwrap();
    assert_eq!(
        unsafe { libc::kill(owner_pid, 0) },
        0,
        "provider group owner is not live"
    );
    // Turn A: one native dynamic tool waits on the original reply pipe.
    let tool_opened_at = Instant::now();
    client
        .execute(envelope(
            &host,
            &grant,
            Some(session),
            uuid::Uuid::new_v4(),
            CanonicalAction::Submit {
                delivery_id: uuid::Uuid::new_v4(),
                turn_id: uuid::Uuid::new_v4(),
                input: serde_json::json!("Call runtime_probe exactly once and wait for its result."),
            },
        ))
        .unwrap();
    let opened = wait_events(&mut client, &grant, session, |events| {
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
    let tool_request = opened
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
    // The real CLI enters runtime ingress under the session grant while
    // the native request is still pending.
    let cli_id = uuid::Uuid::new_v4();
    let cli = Command::new(&helper)
        .args(["models", "--output", "json"])
        .env("GODDARD_RUNTIME_INGRESS", &descriptor)
        .env("GODDARD_TASK_ID", task.to_string())
        .env("GODDARD_RUNTIME_REQUEST_ID", cli_id.to_string())
        .env_remove("GODDARD_DAEMON_ADDRESS")
        .env_remove("GODDARD_AGENT_TOKEN")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // Barrier: the request is durably recorded before the storm begins.
    {
        let mut agent = RuntimeClient::agent(&host.root, &agent_grant).unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Ok(record) = agent.ingress_query(cli_id) {
                assert_eq!(record.disposition, IngressDisposition::Pending);
                assert_eq!(record.session, session);
                let now_ms = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_millis() as u64;
                assert!(
                    record.deadline_ms > now_ms && record.deadline_ms <= now_ms + 600_000,
                    "original CLI deadline not preserved: {}",
                    record.deadline_ms
                );
                break;
            }
            assert!(Instant::now() < deadline, "CLI ingress never recorded");
            std::thread::yield_now();
        }
    }
    // 25 controller replacements while a native request and a scoped CLI
    // request both wait: same pid, generation, pending identities.
    for _ in 0..25 {
        drop(client);
        client = host.client();
        grant = client.acquire_control(&ClientHello::current()).unwrap();
        let inventory = client.reconcile(grant.epoch).unwrap();
        let entry = &inventory.sessions[0];
        assert_eq!(entry.session, session);
        assert_eq!(entry.status, SessionStatus::Live);
        assert!(entry.adapter_alive);
        assert!(entry.pending_requests.contains(&tool_request));
        assert!(entry.pending_ingress.contains(&cli_id));
        assert_eq!(unsafe { libc::kill(owner_pid, 0) }, 0);
    }
    // The new epoch's service drains the original request identity and
    // deadline exactly once; a repeated drain cannot re-dispatch it.
    let mut service =
        RuntimeClient::service(&host.root, &host.token, grant.epoch, &ClientHello::current())
            .unwrap()
            .0;
    let forwarded = service.service_drain(grant.epoch, 0).unwrap();
    let forwarded = forwarded
        .iter()
        .find(|r| r.request_id == cli_id)
        .expect("CLI request was not forwarded");
    assert_eq!(forwarded.session, session);
    assert_eq!(forwarded.class, IngressKind::Cli);
    service
        .service_result(
            grant.epoch,
            cli_id,
            ForwardedOutcome::Completed {
                result: serde_json::json!({"type":"agentModelOptions","options":[]}),
            },
        )
        .unwrap();
    let output = cli.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap(),
        serde_json::json!({"options":[]})
    );
    // Deadline and identity boundaries under the real grant: an already
    // dead request expires on arrival, a reused identity with a changed
    // payload is refused, and an uncertain outcome reconciles without
    // resend.
    let mut agent = RuntimeClient::agent(&host.root, &agent_grant).unwrap();
    let future_ms = || {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
            + 600_000
    };
    let dead = agent
        .ingress(IngressRequest {
            request_id: uuid::Uuid::new_v4(),
            class: IngressKind::Cli,
            subject: serde_json::json!({"type":"agentListModels"}),
            deadline_ms: 1,
        })
        .unwrap();
    assert_eq!(dead.disposition, IngressDisposition::Expired);
    let mismatched = agent.ingress(IngressRequest {
        request_id: cli_id,
        class: IngressKind::Cli,
        subject: serde_json::json!({"type":"agentListCommands"}),
        deadline_ms: future_ms(),
    });
    assert!(matches!(
        mismatched,
        Err(ClientError::Host(HostError::PayloadMismatch))
    ));
    let uncertain_id = uuid::Uuid::new_v4();
    let pending = agent
        .ingress(IngressRequest {
            request_id: uncertain_id,
            class: IngressKind::Cli,
            subject: serde_json::json!({"type":"agentListModels"}),
            deadline_ms: future_ms(),
        })
        .unwrap();
    assert_eq!(pending.disposition, IngressDisposition::Pending);
    assert!(
        service
            .service_drain(grant.epoch, 0)
            .unwrap()
            .iter()
            .any(|r| r.request_id == uncertain_id)
    );
    let reconciled = service
        .service_result(
            grant.epoch,
            uncertain_id,
            ForwardedOutcome::Uncertain {
                detail: "effect may have occurred".into(),
            },
        )
        .unwrap();
    assert!(matches!(
        reconciled.disposition,
        IngressDisposition::Uncertain { .. }
    ));
    assert!(
        !service
            .service_drain(grant.epoch, 0)
            .unwrap()
            .iter()
            .any(|r| r.request_id == uncertain_id),
        "an uncertain request must never be redispatched"
    );
    // Unknown request identity is refused, not fabricated.
    assert!(matches!(
        client
            .execute(envelope(
                &host,
                &grant,
                Some(session),
                uuid::Uuid::new_v4(),
                CanonicalAction::ResolveRequest {
                    request_id: "no-such-native-request".into(),
                    resolution: serde_json::json!({"decision":"accept"}),
                },
            ))
            .unwrap()
            .disposition,
        CommandDisposition::Rejected { .. }
    ));
    // One reply on the original pipe; the second identical command returns
    // the recorded truth instead of writing again.
    let resolve = CanonicalAction::ResolveRequest {
        request_id: tool_request.clone(),
        resolution: serde_json::json!({"success":true,"contentItems":[{"type":"inputText","text":"stable-runtime-scoped-probe"}]}),
    };
    let receipt = client
        .execute(envelope(
            &host,
            &grant,
            Some(session),
            uuid::Uuid::new_v4(),
            resolve.clone(),
        ))
        .unwrap();
    let events = wait_events(&mut client, &grant, session, |events| {
        events
            .iter()
            .any(|e| matches!(e.event, ProviderEvent::TurnCompleted { .. }))
    });
    let duplicate = client
        .execute(envelope(
            &host,
            &grant,
            Some(session),
            receipt.command_id,
            resolve,
        ))
        .unwrap();
    assert!(
        duplicate.disposition == receipt.disposition
            || matches!(
                duplicate.disposition,
                CommandDisposition::Terminal {
                    outcome: TerminalOutcome::Completed,
                    event_sequence: Some(_)
                }
            ),
        "duplicate resolve must return recorded truth: {duplicate:?} vs {receipt:?}"
    );
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e.event, ProviderEvent::TurnCompleted { .. }))
            .count(),
        1
    );
    assert!(events.iter().any(
        |e| matches!(&e.event,ProviderEvent::RequestAnswered{request_id} if request_id==&tool_request)
    ));
    eprintln!(
        "actual Codex native tool held {}ms across 25 controller replacements on pid {owner_pid} thread {thread_id}",
        tool_opened_at.elapsed().as_millis()
    );
    // Replay is deterministic across another replacement: identical
    // ordered envelopes from the same cursor.
    let first = events;
    drop(client);
    client = host.client();
    grant = client.acquire_control(&ClientHello::current()).unwrap();
    client.reconcile(grant.epoch).unwrap();
    let second = client
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
        .events;
    assert_eq!(first, second);
    assert!(second.windows(2).all(|w| w[1].sequence == w[0].sequence + 1));
    // Turn B: the model executes the pinned goddard-agent inside the
    // provider's own environment. A write op makes the exec require a
    // native approval under the on-request policy; accepting escalates it,
    // so the in-provider CLI can persist and reach ingress while more
    // controller replacements land.
    let mut in_provider_cli: Option<uuid::Uuid> = None;
    client
        .execute(envelope(
            &host,
            &grant,
            Some(session),
            uuid::Uuid::new_v4(),
            CanonicalAction::Submit {
                delivery_id: uuid::Uuid::new_v4(),
                turn_id: uuid::Uuid::new_v4(),
                input: serde_json::json!(format!(
                    "Run exactly this shell command and nothing else: touch {}/probe.marker && {} models\nThen reply with the raw stdout of the goddard-agent command.",
                    workspace.display(),
                    helper.display()
                )),
            },
        ))
        .unwrap();
    let approval_wait = Instant::now();
    let deadline = Instant::now() + Duration::from_secs(180);
    let mut resolved_requests: Vec<String> = Vec::new();
    let mut service =
        RuntimeClient::service(&host.root, &host.token, grant.epoch, &ClientHello::current())
            .unwrap()
            .0;
    while in_provider_cli.is_none() {
        let events = client
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
            .events;
        // Answer each native request once, on its original identity: the
        // pinned CLI command is approved; anything else is declined so the
        // turn cannot stall on an unexpected prompt.
        for (request_id, decision) in events
            .iter()
            .filter_map(|e| match &e.event {
                ProviderEvent::RequestOpened {
                    request_id,
                    class: RequestClass::Approval,
                    subject,
                    ..
                } if !resolved_requests.contains(request_id) => Some((
                    request_id.clone(),
                    if subject.to_string().contains("goddard-agent")
                        || subject.to_string().contains("probe.marker")
                    {
                        "accept"
                    } else {
                        "decline"
                    },
                )),
                _ => None,
            })
            .collect::<Vec<_>>()
        {
            client
                .execute(envelope(
                    &host,
                    &grant,
                    Some(session),
                    uuid::Uuid::new_v4(),
                    CanonicalAction::ResolveRequest {
                        request_id: request_id.clone(),
                        resolution: serde_json::json!({"decision":decision}),
                    },
                ))
                .unwrap();
            resolved_requests.push(request_id);
        }
        // Non-blocking input requests cannot be declined; answer empty
        // once so the provider is never stuck on the harness.
        for request_id in events
            .iter()
            .filter_map(|e| match &e.event {
                ProviderEvent::RequestOpened {
                    request_id,
                    class: RequestClass::Input,
                    ..
                } if !resolved_requests.contains(request_id) => Some(request_id.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
        {
            client
                .execute(envelope(
                    &host,
                    &grant,
                    Some(session),
                    uuid::Uuid::new_v4(),
                    CanonicalAction::ResolveRequest {
                        request_id: request_id.clone(),
                        resolution: serde_json::json!({"answers":{}}),
                    },
                ))
                .unwrap();
            resolved_requests.push(request_id);
        }
        let turn_b_ended = events
            .iter()
            .any(|e| matches!(e.event, ProviderEvent::Terminated { .. }))
            || events
                .iter()
                .filter(|e| matches!(e.event, ProviderEvent::TurnCompleted { .. }))
                .count()
                >= 2;
        if turn_b_ended {
            break;
        }
        if let Some(request) = service
            .service_drain(grant.epoch, 0)
            .unwrap()
            .into_iter()
            .find(|r| {
                r.session == session
                    && r.class == IngressKind::Cli
                    && r.subject["type"] == "agentListModels"
                    && r.request_id != cli_id
                    && r.request_id != uncertain_id
            })
        {
            in_provider_cli = Some(request.request_id);
            break;
        }
        assert!(
            Instant::now() < deadline,
            "provider never ran the pinned CLI (approval or exec missing)"
        );
        std::thread::yield_now();
    }
    if in_provider_cli.is_none() {
        // The in-provider CLI never reached ingress. This must be the
        // honest closed boundary — never a silent fallback onto daemon
        // credentials: read-only sandboxing cannot persist the request
        // intent, and a declined approval skips the exec entirely.
        let events = client
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
            .events;
        let exec_items: Vec<serde_json::Value> = events
            .iter()
            .filter_map(|e| match &e.event {
                ProviderEvent::TurnDelta { delta, .. }
                    if delta["method"] == "item/completed"
                        && delta["params"]["item"]["type"] == "commandExecution" =>
                {
                    Some(delta["params"]["item"].clone())
                }
                _ => None,
            })
            .collect();
        let ran_cli = exec_items.iter().find(|item| {
            item["command"].as_str().is_some_and(|c| c.contains("goddard-agent"))
        });
        let Some(item) = ran_cli else {
            let methods: Vec<String> = events
                .iter()
                .filter_map(|e| match &e.event {
                    ProviderEvent::TurnDelta { delta, .. } => {
                        delta["method"].as_str().map(str::to_owned)
                    }
                    _ => None,
                })
                .collect();
            panic!("provider never ran the pinned CLI; methods={methods:?}");
        };
        let output = item["aggregatedOutput"].as_str().unwrap_or_default();
        assert!(
            output.contains("stable ingress request intent cannot be persisted")
                || output.contains("stable ingress request storage")
                || output.contains("stable ingress authentication unavailable")
                || output.contains("stable ingress task binding"),
            "in-provider CLI must fail closed without daemon fallback, got: {output}"
        );
        eprintln!(
            "actual Codex in-provider CLI under read-only sandbox failed closed as designed: {}",
            &output[..output.len().min(240)]
        );
    }
    if let Some(in_provider_id) = in_provider_cli {
        // The provider's own CLI call waits through more controller
        // replacement without redispatch or a new identity.
        for _ in 0..5 {
            drop(client);
            client = host.client();
            grant = client.acquire_control(&ClientHello::current()).unwrap();
            let inventory = client.reconcile(grant.epoch).unwrap();
            assert!(inventory.sessions[0]
                .pending_ingress
                .contains(&in_provider_id));
            service = RuntimeClient::service(
                &host.root,
                &host.token,
                grant.epoch,
                &ClientHello::current(),
            )
            .unwrap()
            .0;
            assert!(
                service
                    .service_drain(grant.epoch, 0)
                    .unwrap()
                    .iter()
                    .any(|r| r.request_id == in_provider_id),
                "forwarded request must remain drainable under the new epoch"
            );
        }
        service
            .service_result(
                grant.epoch,
                in_provider_id,
                ForwardedOutcome::Completed {
                    result: serde_json::json!({"type":"agentModelOptions","options":[]}),
                },
            )
            .unwrap();
        wait_events(&mut client, &grant, session, |events| {
            events
                .iter()
                .filter(|e| matches!(e.event, ProviderEvent::TurnCompleted { .. }))
                .count()
                == 2
        });
        eprintln!(
            "actual Codex approval boundary + in-provider CLI qualified: approval waited {}ms across {} native request resolutions, CLI request {in_provider_id} held through 5 further epochs ({version})",
            approval_wait.elapsed().as_millis(),
            resolved_requests.len()
        );
    } else {
        eprintln!(
            "actual Codex approval/exec boundary qualified: turn B ended with {} resolved native requests ({version})",
            resolved_requests.len()
        );
    }
    // The same provider owner still holds the session; stop terminates it.
    assert_eq!(unsafe { libc::kill(owner_pid, 0) }, 0);
    client
        .execute(envelope(
            &host,
            &grant,
            Some(session),
            uuid::Uuid::new_v4(),
            CanonicalAction::StopSession,
        ))
        .unwrap();
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
    let deadline = Instant::now() + Duration::from_secs(10);
    while unsafe { libc::kill(owner_pid, 0) } == 0 {
        assert!(
            Instant::now() < deadline,
            "provider group owner survived confirmed stop"
        );
        std::thread::yield_now();
    }
    eprintln!(
        "actual Codex scoped ingress qualified: runtime={} thread={thread_id} pid={owner_pid} replacements=30 {version}",
        host.endpoint.instance
    );
}
