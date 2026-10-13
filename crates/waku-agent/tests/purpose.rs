//! Purpose must cross the real CLI boundary without inventing access intent.
use std::{net::TcpListener, process::Command, time::Duration};
use uuid::Uuid;
use waku_protocol::{
    ClientMessage, Command as WireCommand, PROTOCOL_VERSION, ResponseOutcome, ResponsePayload,
    ServerMessage,
    boss::{BossOperation, BossResult, EmployeeControl, EmployeePurpose, PlanAssignmentKind},
};

#[test]
fn purpose_cli_preserves_explicit_intent_and_classifies_existing_work() {
    let employee_id = Uuid::new_v4().to_string();
    for (args, expected_kind) in [
        (
            vec![
                "boss",
                "summon",
                "--title",
                "Chore",
                "--text",
                "Check the repo",
                "--project",
                "/repo",
                "--purpose",
                "helping",
            ],
            None,
        ),
        (
            vec![
                "boss",
                "summon",
                "--title",
                "Chore",
                "--text",
                "Check the repo",
                "--project",
                "/repo",
                "--purpose",
                "helping",
                "--assignment-kind",
                "implementation",
            ],
            Some(PlanAssignmentKind::Implementation),
        ),
        (
            vec![
                "boss",
                "summon",
                "--persona",
                "Builder",
                "--title",
                "Feature",
                "--text",
                "Build",
                "--project",
                "/repo",
            ],
            None,
        ),
        (
            vec!["boss", "employee", "purpose", &employee_id, "helping"],
            None,
        ),
        (
            vec![
                "boss",
                "persona",
                "upsert",
                "--new",
                "--json",
                r#"{"name":"Builder","markdown":"Build","defaultPurpose":"building"}"#,
            ],
            None,
        ),
    ] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap().to_string();
        let control = args[1] == "employee";
        let persona = args[1] == "persona";
        let persona_summon = args.contains(&"--persona");
        let daemon = std::thread::spawn(move || {
            for index in 0..if persona_summon { 2 } else { 1 } {
                let deadline = std::time::Instant::now() + Duration::from_secs(5);
                let stream = loop {
                    match listener.accept() {
                        Ok((stream, _)) => break stream,
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            assert!(std::time::Instant::now() < deadline, "CLI never connected");
                            std::thread::sleep(Duration::from_millis(5));
                        }
                        Err(error) => panic!("accept: {error}"),
                    }
                };
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut socket = tungstenite::accept(stream).unwrap();
                let _: ClientMessage =
                    serde_json::from_str(socket.read().unwrap().to_text().unwrap()).unwrap();
                socket
                    .send(tungstenite::Message::Text(
                        serde_json::to_string(&ServerMessage::Hello {
                            protocol_version: PROTOCOL_VERSION,
                            daemon_version: "test".into(),
                            daemon_commit: None,
                            agent_cli_available: true,
                        })
                        .unwrap()
                        .into(),
                    ))
                    .unwrap();
                let ClientMessage::Request(request) =
                    serde_json::from_str(socket.read().unwrap().to_text().unwrap()).unwrap()
                else {
                    panic!("expected request");
                };
                let WireCommand::Boss { operation } = request.command else {
                    panic!("expected Boss operation");
                };
                let result = if persona_summon && index == 0 {
                    assert!(matches!(operation, BossOperation::View));
                    BossResult::State { state: serde_json::from_value(serde_json::json!({
                    "identity": {"id": Uuid::new_v4(), "name":"Boss", "avatarSeed":"boss"},
                    "personaId": Uuid::new_v4(), "sessionId": null, "employees":[], "revision":0,
                    "personas": [{"id": Uuid::new_v4(), "name":"Builder", "markdown":"Build", "permissions":{}, "defaultPurpose":"building"}]
                })).unwrap() }
                } else {
                    if persona_summon {
                        assert!(matches!(
                            operation,
                            BossOperation::Summon {
                                persona_id: Some(_),
                                purpose: None,
                                assignment_kind: None,
                                ..
                            }
                        ));
                    } else if persona {
                        assert!(matches!(operation, BossOperation::UpsertPersona { persona }
                    if persona.default_purpose == Some(Some(EmployeePurpose::Building))));
                    } else if control {
                        assert!(matches!(
                            operation,
                            BossOperation::Control {
                                action: EmployeeControl::SetPurpose {
                                    purpose: EmployeePurpose::Helping
                                },
                                ..
                            }
                        ));
                    } else {
                        assert!(matches!(operation, BossOperation::Summon {
                    purpose: Some(EmployeePurpose::Helping), assignment_kind, ..
                } if assignment_kind == expected_kind));
                    }
                    BossResult::Saved
                };
                socket
                    .send(tungstenite::Message::Text(
                        serde_json::to_string(&ServerMessage::Response {
                            request_id: request.request_id,
                            outcome: ResponseOutcome::Ok {
                                payload: ResponsePayload::Boss { result },
                            },
                        })
                        .unwrap()
                        .into(),
                    ))
                    .unwrap();
            }
        });
        let output = Command::new(env!("CARGO_BIN_EXE_goddard-agent"))
            .args(args)
            .env("GODDARD_DAEMON_ADDRESS", address)
            .env("GODDARD_AGENT_TOKEN", "test-purpose-token")
            .env("GODDARD_TASK_ID", Uuid::new_v4().to_string())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        daemon.join().unwrap();
    }
}
