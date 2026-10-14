//! Run the real script CLI against an isolated transport and durable Boss
//! service. No live daemon restart or production Boss mutations are needed.
use std::{net::TcpListener, process::Command, time::Duration};
use uuid::Uuid;
use waku_boss::boss::BossService;
use waku_protocol::{
    ClientMessage, Command as WireCommand, PROTOCOL_VERSION, ResponseOutcome, ResponsePayload,
    ServerMessage, boss::BossOperation,
};

#[test]
fn boss_script_cli_returns_small_acknowledgements_for_multiple_committed_ops() {
    let root = std::env::temp_dir().join(format!("boss-script-cli-{}", Uuid::new_v4()));
    let service = BossService::open(root.clone()).unwrap();
    service.activate().unwrap();
    // Each string fits Rhai's 4 MiB limit, but their aggregate does not.
    for title in ["x", "y"] {
        service
            .create_outcome(None, &title.repeat(2 * 1024 * 1024 + 1), "seed")
            .unwrap();
    }
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let address = listener.local_addr().unwrap().to_string();
    let server = std::thread::spawn(move || {
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
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
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        let mut socket = tungstenite::accept(stream).unwrap();
        let hello: ClientMessage =
            serde_json::from_str(socket.read().unwrap().to_text().unwrap()).unwrap();
        assert!(
            matches!(hello, ClientMessage::Hello { token, .. } if token == "test-script-token")
        );
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
        let WireCommand::Boss {
            operation: BossOperation::Eval { script },
        } = request.command
        else {
            panic!("expected Boss script");
        };
        let result = service
            .eval(Uuid::nil(), &script, &|operation| {
                service.handle(None, operation)
            })
            .unwrap();
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
        assert_eq!(service.document().outcomes.len(), 4);
        drop(service);
    });
    let output = Command::new(env!("CARGO_BIN_EXE_goddard-agent"))
        .args([
            "boss",
            "script",
            "--text",
            r#"
            let first = op(#{type:"createOutcome", outcome:"first", successCriteria:"done"});
            let second = op(#{type:"createOutcome", outcome:"second", successCriteria:"done"});
            #{first:first,second:second,digest:view()}
        "#,
        ])
        .env("GODDARD_DAEMON_ADDRESS", address)
        .env("GODDARD_AGENT_TOKEN", "test-script-token")
        .env("GODDARD_TASK_ID", Uuid::new_v4().to_string())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    server.join().unwrap();
    let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["type"], "eval");
    assert_eq!(result["value"]["first"]["accepted"], true);
    assert_eq!(result["value"]["second"]["accepted"], true);
    assert_ne!(
        result["value"]["first"]["outcomeId"],
        result["value"]["second"]["outcomeId"]
    );
    assert_eq!(result["value"]["digest"]["counts"]["outcomes"], 4);
    assert!(output.stdout.len() < 1024);
    // Keep measured CLI evidence in the test transcript.
    println!(
        "boss script: {} bytes; two accepted mutations; four persisted outcomes",
        output.stdout.len()
    );
    let reopened = BossService::open(root.clone()).unwrap();
    assert_eq!(reopened.document().outcomes.len(), 4);
    drop(reopened);
    std::fs::remove_dir_all(root).unwrap();
}
