//! Exercise documented chain selectors through the actual CLI and wire response.
use serde_json::Value;
use std::{
    net::TcpListener,
    process::Command,
    time::{Duration, Instant},
};
use waku_protocol::{
    ClientMessage, Command as WireCommand, PROTOCOL_VERSION, ResponseOutcome, ResponsePayload,
    ServerMessage,
    boss::{BossOperation, BossResult},
    chains::ChainOperation,
};

#[test]
fn chain_guide_create_and_revise_preserve_selectors_and_actionable_errors() {
    let schema = Command::new(env!("CARGO_BIN_EXE_goddard-agent"))
        .args(["boss", "chain", "--schema"])
        .output()
        .unwrap();
    assert!(schema.status.success());
    let schema: Value = serde_json::from_slice(&schema.stdout).unwrap();
    let step = &schema["definitions"]["ChainStepDefinition"];
    assert_eq!(step["oneOf"].as_array().unwrap().len(), 2);
    assert_eq!(step["properties"]["projectId"]["format"], "uuid");
    assert!(
        !step["required"]
            .as_array()
            .unwrap()
            .contains(&Value::String("projectId".into()))
    );
    let guide = include_str!("../../../docs/boss-projects.md");
    let examples_from = |guide: &str| -> Vec<Value> {
        guide
            .split("```json")
            .skip(1)
            .filter_map(|block| {
                let block = block.split("```").next().unwrap();
                let value: Value = serde_json::from_str(block).unwrap();
                matches!(value["type"].as_str(), Some("create" | "revise")).then_some(value)
            })
            .collect()
    };
    let examples = examples_from(guide);
    let crlf = guide.replace("\r\n", "\n").replace('\n', "\r\n");
    assert_eq!(examples_from(&crlf), examples);
    assert_eq!(examples.len(), 2);
    for example in examples {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap().to_string();
        let expected: ChainOperation = serde_json::from_value(example.clone()).unwrap();
        let server = std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(10);
            let stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(Instant::now() < deadline, "CLI never connected");
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    Err(error) => panic!("accept: {error}"),
                }
            };
            stream.set_nonblocking(false).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(10)))
                .unwrap();
            let mut socket = tungstenite::accept(stream).unwrap();
            let _: ClientMessage =
                serde_json::from_str(socket.read().unwrap().to_text().unwrap()).unwrap();
            socket
                .send(tungstenite::Message::Text(
                    serde_json::to_string(&ServerMessage::Hello {
                        protocol_version: PROTOCOL_VERSION,
                        daemon_version: "isolated-chain-test".into(),
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
                panic!("request expected")
            };
            let WireCommand::Boss {
                operation: BossOperation::Chain { action },
            } = request.command
            else {
                panic!("typed chain operation expected")
            };
            assert_eq!(
                serde_json::to_value(&action).unwrap(),
                serde_json::to_value(&expected).unwrap()
            );
            let (mut step, index) = match action {
                ChainOperation::Create { mut definition, .. } => (definition.steps.remove(0), 0),
                ChainOperation::Revise {
                    definition, step, ..
                } => (definition, step),
                _ => panic!("create/revise expected"),
            };
            assert!(step.project_id.is_nil());
            let error = step.resolve_project(&[], index).unwrap_err();
            socket
                .send(tungstenite::Message::Text(
                    serde_json::to_string(&ServerMessage::Response {
                        request_id: request.request_id,
                        outcome: ResponseOutcome::Ok {
                            payload: ResponsePayload::Boss {
                                result: BossResult::ProjectRejected { error },
                            },
                        },
                    })
                    .unwrap()
                    .into(),
                ))
                .unwrap();
        });
        let output = Command::new(env!("CARGO_BIN_EXE_goddard-agent"))
            .args([
                "boss",
                "chain",
                "--json",
                &example.to_string(),
                "--output",
                "json",
            ])
            .env("GODDARD_DAEMON_ADDRESS", address)
            .env("GODDARD_AGENT_TOKEN", "isolated-chain-test")
            .env_remove("GODDARD_RUNTIME_INGRESS")
            .output()
            .unwrap();
        server.join().unwrap();
        assert_eq!(output.status.code(), Some(2));
        let error: Value = serde_json::from_slice(&output.stderr).unwrap();
        assert_eq!(error["error"]["code"], "project_unknown");
        assert!(error["error"]["projectFailure"]["step"].is_number());
        assert!(
            error["error"]["projectFailure"]["nextAction"]
                .as_str()
                .unwrap()
                .contains("project")
        );
    }
}
