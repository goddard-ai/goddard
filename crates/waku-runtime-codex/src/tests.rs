use super::*;
use std::{fs, time::Instant};
use waku_runtime::{
    ActionEnvelope, ClientHello, CommandDisposition, ControlGrant, EventCursor, EventEnvelope,
    HostError, HostOptions, RuntimeHost, SessionRef, SessionStatus, SystemClock,
};

struct Rig {
    root: PathBuf,
    host: RuntimeHost,
    control: ControlGrant,
    token: String,
    session: Uuid,
}
impl Rig {
    fn new(real: bool) -> Self {
        Self::build(real, None)
    }
    fn build(real: bool, agent: Option<AgentIngress>) -> Self {
        let root = std::env::temp_dir().join(format!("runtime-codex-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let binary = if real {
            PathBuf::from(std::env::var("CODEX_BINARY").expect("set CODEX_BINARY"))
        } else {
            let binary = root.join("codex-fixture");
            fs::write(&binary, FIXTURE).unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&binary, fs::Permissions::from_mode(0o700)).unwrap();
            }
            binary
        };
        let mut host = RuntimeHost::open(
            &root.join("journal"),
            HostOptions::new("codex-test"),
            Arc::new(CodexFactory),
            Box::new(SystemClock),
        )
        .unwrap();
        let token = host.credentials().controller.clone();
        let control = host
            .acquire_control(&token, &ClientHello::current())
            .unwrap();
        let session = Uuid::new_v4();
        let launch = CodexLaunch {
            binary,
            cwd: root.clone(),
            approval_policy: "on-request".into(),
            sandbox: "read-only".into(),
            developer_instructions: None,
            agent,
            dynamic_tools: vec![json!({"type":"function","name":"runtime_probe",
                "description":"Return the stable-runtime probe token. Call exactly once when instructed.",
                "inputSchema":{"type":"object","properties":{},"additionalProperties":false}})],
        };
        let mut rig = Self {
            root,
            host,
            control,
            token,
            session,
        };
        let receipt = rig.execute(
            Uuid::new_v4(),
            CanonicalAction::AdmitSession {
                session_id: session,
                adapter: "codex".into(),
                launch: serde_json::to_value(launch).unwrap(),
                requested: RequestedConfig {
                    provider: Some("codex".into()),
                    ..RequestedConfig::default()
                },
            },
            false,
        );
        assert!(
            matches!(receipt, CommandDisposition::Terminal { .. }),
            "admission: {receipt:?}"
        );
        rig
    }
    fn envelope(&self, id: Uuid, action: CanonicalAction, session: bool) -> ActionEnvelope {
        ActionEnvelope {
            namespace: "codex-test".into(),
            runtime_instance: self.host.instance(),
            epoch: self.control.epoch,
            retry_window_id: self.control.retry_window_id,
            command_id: id,
            session: session.then_some(SessionRef {
                session_id: self.session,
                generation: 1,
            }),
            action,
        }
    }
    fn execute(&mut self, id: Uuid, action: CanonicalAction, session: bool) -> CommandDisposition {
        let env = self.envelope(id, action, session);
        self.host.execute(&self.token, env).unwrap().disposition
    }
    fn events(&mut self) -> Vec<EventEnvelope> {
        self.host
            .events(
                &self.token,
                self.control.epoch,
                EventCursor {
                    session_id: self.session,
                    generation: 1,
                    applied: 0,
                },
            )
            .unwrap()
            .events
    }
    fn until(&mut self, condition: impl Fn(&[EventEnvelope]) -> bool) -> Vec<EventEnvelope> {
        let deadline = Instant::now() + Duration::from_secs(120);
        loop {
            let events = self.events();
            if condition(&events) {
                return events;
            }
            assert!(
                !events.iter().any(|e| matches!(
                    e.event,
                    ProviderEvent::ProviderFailed { .. } | ProviderEvent::Terminated { .. }
                )),
                "provider ended before barrier ({} events)",
                events.len()
            );
            assert!(
                Instant::now() < deadline,
                "provider barrier timed out ({} events)",
                events.len()
            );
            thread::yield_now();
        }
    }
    fn submit(&mut self, text: &str) -> (Uuid, Uuid, CanonicalAction) {
        let id = Uuid::new_v4();
        let turn = Uuid::new_v4();
        let action = CanonicalAction::Submit {
            delivery_id: Uuid::new_v4(),
            turn_id: turn,
            input: json!(text),
        };
        assert!(matches!(
            self.execute(id, action.clone(), true),
            CommandDisposition::ProviderAccepted
        ));
        (id, turn, action)
    }
    fn reconnect(&mut self) {
        self.control = self
            .host
            .acquire_control(&self.token, &ClientHello::current())
            .unwrap();
    }
}
impl Drop for Rig {
    fn drop(&mut self) {
        let emergency = self.host.credentials().emergency.clone();
        let _ = self.host.emergency_stop(&emergency, self.session);
        let _ = fs::remove_dir_all(&self.root);
    }
}
fn opened(events: &[EventEnvelope], class: RequestClass) -> Option<String> {
    events.iter().find_map(|e| match &e.event {
        ProviderEvent::RequestOpened {
            request_id,
            class: c,
            ..
        } if *c == class => Some(request_id.clone()),
        _ => None,
    })
}
const FIXTURE: &str = include_str!("../tests/app_server_fixture.py");

#[test]
fn original_pipe_callbacks_survive_25_controller_transfers_and_deduplicate() {
    let mut rig = Rig::new(false);
    let (command, turn, submit) = rig.submit("callbacks");
    let events = rig.until(|events| opened(events, RequestClass::Tool).is_some());
    let tool = opened(&events, RequestClass::Tool).unwrap();
    assert_eq!(tool, "11");
    let old = rig.envelope(Uuid::new_v4(), CanonicalAction::StopSession, true);
    let instance = rig.host.instance();
    for _ in 0..25 {
        rig.reconnect();
        assert_eq!(rig.host.instance(), instance);
        assert!(matches!(
            rig.execute(command, submit.clone(), true),
            CommandDisposition::ProviderAccepted
        ));
    }
    assert!(matches!(
        rig.host.execute(&rig.token, old),
        Err(HostError::StaleEpoch { .. })
    ));
    for (class, resolution) in [
        (
            RequestClass::Tool,
            json!({"success":true,"contentItems":[]}),
        ),
        (RequestClass::Approval, json!({"decision":"decline"})),
        (RequestClass::Input, json!({"answers":{}})),
    ] {
        let events = rig.until(|events| opened(events, class).is_some());
        let request = opened(&events, class).unwrap();
        rig.reconnect();
        let command = Uuid::new_v4();
        let action = CanonicalAction::ResolveRequest {
            request_id: request.clone(),
            resolution,
        };
        let _ = rig.execute(command, action.clone(), true);
        if class == RequestClass::Tool {
            rig.until(|events|events.iter().any(|e|matches!(&e.event,ProviderEvent::RequestAnswered {request_id} if request_id==&request)));
        }
        rig.reconnect();
        let disposition = rig.execute(command, action, true);
        if class == RequestClass::Tool {
            assert!(matches!(disposition, CommandDisposition::Terminal { .. }));
        } else {
            assert!(matches!(disposition, CommandDisposition::Uncertain { .. }));
        }
    }
    let events = rig.until(|events| {
        events
            .iter()
            .any(|e| matches!(e.event,ProviderEvent::TurnCompleted {turn_id,..} if turn_id==turn))
    });
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
            .all(|w| w[1].sequence == w[0].sequence + 1)
    );
    assert_eq!(events, rig.events());
    assert!(!events.iter().any(|e|matches!(&e.event,ProviderEvent::TurnDelta {delta,..} if delta.to_string().contains("must-ignore-foreign"))));
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e.event, ProviderEvent::RequestAnswered { .. }))
            .count(),
        1
    );
    assert!(events.iter().any(|e|matches!(&e.event,ProviderEvent::TurnDelta {delta,..} if delta["method"]=="turn/started" && delta["params"]["turn"]["id"]=="native-turn")));
    let received = fs::read_to_string(rig.root.join("received.jsonl")).unwrap();
    let messages: Vec<Value> = received
        .lines()
        .map(|s| serde_json::from_str(s).unwrap())
        .collect();
    assert_eq!(
        messages
            .iter()
            .filter(|v| v["method"] == "turn/start")
            .count(),
        1
    );
    assert_eq!(
        messages
            .iter()
            .filter(|v| v.get("method").is_none())
            .count(),
        3
    );
    assert_eq!(
        messages
            .iter()
            .filter(|v| v.get("method").is_none() && v["id"] == json!(11))
            .count(),
        1
    );
    assert_eq!(
        messages
            .iter()
            .filter(|v| v.get("method").is_none() && v["id"] == json!("11"))
            .count(),
        1
    );
}

#[test]
fn native_cancel_holds_later_dispatch_and_stop_prevents_further_work() {
    let mut rig = Rig::new(false);
    let (_, turn, _) = rig.submit("cancel");
    rig.until(|e| {
        e.iter()
            .any(|e| matches!(e.event, ProviderEvent::TurnStarted { .. }))
    });
    let _ = rig.execute(
        Uuid::new_v4(),
        CanonicalAction::CancelTurn { turn_id: turn },
        true,
    );
    rig.until(|e| {
        e.iter()
            .any(|e| matches!(e.event, ProviderEvent::TurnCancelled { .. }))
    });
    let action = CanonicalAction::Submit {
        delivery_id: Uuid::new_v4(),
        turn_id: Uuid::new_v4(),
        input: json!("held"),
    };
    assert_eq!(
        rig.execute(Uuid::new_v4(), action, true),
        CommandDisposition::DispatchPending
    );
    let _ = rig.execute(Uuid::new_v4(), CanonicalAction::StopSession, true);
    // Shared-host disconnect projection is a documented prerequisite defect;
    // this check proves cancellation identity, the hold, and the stop fence.
    assert!(matches!(
        rig.execute(Uuid::new_v4(), CanonicalAction::PermitDispatch, true),
        CommandDisposition::Rejected { .. }
    ));
    let messages = fs::read_to_string(rig.root.join("received.jsonl")).unwrap();
    assert_eq!(
        messages
            .lines()
            .filter(|s| s.contains("\"method\": \"turn/start\""))
            .count(),
        1
    );
    assert!(messages.contains("\"turnId\": \"native-turn\""));
}

#[test]
fn oversized_transport_interrupts_instead_of_allocating_unbounded_history() {
    let mut rig = Rig::new(false);
    let _ = rig.submit("oversize");
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let events = rig.events();
        if events
            .iter()
            .any(|e| matches!(e.event, ProviderEvent::Terminated { .. }))
        {
            break;
        }
        assert!(Instant::now() < deadline);
        thread::yield_now();
    }
    assert_eq!(
        rig.host
            .reconcile(&rig.token, rig.control.epoch)
            .unwrap()
            .sessions[0]
            .status,
        SessionStatus::Interrupted
    );
}

#[test]
#[ignore = "requires CODEX_BINARY and authenticated actual Codex; disposable read-only thread"]
fn actual_codex_tool_survives_controller_replacement() {
    let mut rig = Rig::new(true);
    let (_,turn,_)=rig.submit("Call runtime_probe exactly once, then reply with the token it returns. Do not use any other tools.");
    let events = rig.until(|e| opened(e, RequestClass::Tool).is_some());
    let request = opened(&events, RequestClass::Tool).unwrap();
    for _ in 0..25 {
        rig.reconnect();
    }
    let _=rig.execute(Uuid::new_v4(),CanonicalAction::ResolveRequest {request_id:request.clone(),
        resolution:json!({"success":true,"contentItems":[{"type":"inputText","text":"stable-probe-43fd2b25"}]})},true);
    let events = rig.until(|e| {
        e.iter()
            .any(|e| matches!(e.event,ProviderEvent::TurnCompleted {turn_id,..} if turn_id==turn))
    });
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e.event, ProviderEvent::TurnCompleted { .. }))
            .count(),
        1
    );
    assert!(events.iter().any(
        |e| matches!(&e.event,ProviderEvent::RequestAnswered {request_id} if request_id==&request)
    ));
    assert!(events.iter().any(|e|matches!(&e.event,ProviderEvent::TurnDelta {delta,..} if delta.to_string().contains("stable-probe-43fd2b25"))));
}

#[test]
fn effective_configuration_comes_from_native_settings_at_the_next_turn() {
    let mut rig = Rig::new(false);
    let target = RequestedConfig {
        provider: Some("codex".into()),
        model: Some("probe-model".into()),
        effort: Some("low".into()),
        options: Value::Null,
    };
    assert!(matches!(
        rig.execute(
            Uuid::new_v4(),
            CanonicalAction::Reconfigure {
                revision: 1,
                target: target.clone()
            },
            true
        ),
        CommandDisposition::ProviderAccepted
    ));
    assert!(
        !rig.events()
            .iter()
            .any(|e| matches!(e.event, ProviderEvent::ConfigActivated { revision: 1, .. }))
    );
    let _ = rig.submit("cancel");
    let events = rig.until(|e| {
        e.iter()
            .any(|e| matches!(e.event, ProviderEvent::ConfigActivated { revision: 1, .. }))
    });
    assert!(events.iter().any(|e|matches!(&e.event,ProviderEvent::ConfigActivated {revision:1,effective,..} if effective.model==target.model && effective.effort==target.effort && effective.provider==target.provider && effective.options["codexThreadId"]=="fixture-thread")));
    assert!(matches!(
        rig.execute(
            Uuid::new_v4(),
            CanonicalAction::Reconfigure {
                revision: 2,
                target: RequestedConfig {
                    options: json!({"sandbox":"danger-full-access"}),
                    ..target
                }
            },
            true
        ),
        CommandDisposition::Rejected { .. }
    ));
}

#[test]
fn codex_child_env_carries_only_the_provisioned_ingress_pairs() {
    let rig = Rig::build(
        false,
        Some(AgentIngress {
            env: [
                ("GODDARD_RUNTIME_INGRESS", "/private/ingress.json"),
                ("GODDARD_TASK_ID", "task-1"),
            ]
            .into_iter()
            .map(|(name, value)| (name.to_string(), value.to_string()))
            .collect(),
            cli_dir: None,
        }),
    );
    // The fixture records every GODDARD_*/WAKU_* name it inherited — the
    // provisioned pairs are the only ones the child may see.
    let inherited: Vec<String> =
        serde_json::from_slice(&fs::read(rig.root.join("provider-environment.json")).unwrap())
            .unwrap();
    assert!(inherited.contains(&"GODDARD_RUNTIME_INGRESS".to_string()));
    assert!(inherited.contains(&"GODDARD_TASK_ID".to_string()));
    assert!(
        !inherited.contains(&"GODDARD_AGENT_TOKEN".to_string()),
        "the daemon token never reaches the provider: {inherited:?}"
    );
    assert!(!inherited.contains(&"GODDARD_DAEMON_ADDRESS".to_string()));
}
