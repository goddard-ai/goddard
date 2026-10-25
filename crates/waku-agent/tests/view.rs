//! `boss view` keeps the daemon's full `BossResult::State` but prints a
//! bounded digest by default, one named slice per SECTION, or the complete
//! state under `--full`. Verified against the real CLI binary and a stub
//! daemon serving a durable `BossService`.

use std::{
    net::TcpListener,
    process::{Command, Output},
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};
use uuid::Uuid;
use waku_boss::boss::BossService;
use waku_protocol::{
    ClientMessage, Command as WireCommand, PROTOCOL_VERSION, ResponseOutcome, ResponsePayload,
    ServerMessage, boss::BossOperation,
};

fn cli(address: &str, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_goddard-agent"))
        .args(["--output", "json"].into_iter().chain(args.iter().copied()))
        .env("GODDARD_DAEMON_ADDRESS", address)
        .env("GODDARD_AGENT_TOKEN", "test-view-token")
        .env("GODDARD_TASK_ID", Uuid::new_v4().to_string())
        .output()
        .unwrap()
}

#[test]
fn boss_view_defaults_to_a_digest_and_serves_named_slices() {
    let root = std::env::temp_dir().join(format!("boss-view-cli-{}", Uuid::new_v4()));
    let service = BossService::open(root.clone()).unwrap();
    service.activate().unwrap();
    service
        .create_outcome(None, "compact view outcome", "seed")
        .unwrap();

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let address = listener.local_addr().unwrap().to_string();
    let done = AtomicBool::new(false);

    std::thread::scope(|scope| {
        let service = &service;
        let done = &done;
        scope.spawn(move || {
            let deadline = std::time::Instant::now() + Duration::from_secs(20);
            while !done.load(Ordering::Relaxed) {
                let (stream, _) = match listener.accept() {
                    Ok(pair) => pair,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(
                            std::time::Instant::now() < deadline,
                            "CLI never connected"
                        );
                        std::thread::sleep(Duration::from_millis(5));
                        continue;
                    }
                    Err(error) => panic!("accept: {error}"),
                };
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(10)))
                    .unwrap();
                let mut socket = tungstenite::accept(stream).unwrap();
                let hello: ClientMessage =
                    serde_json::from_str(socket.read().unwrap().to_text().unwrap()).unwrap();
                assert!(
                    matches!(hello, ClientMessage::Hello { token, .. } if token == "test-view-token")
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
                    operation: operation @ BossOperation::View,
                } = request.command
                else {
                    panic!("expected Boss view");
                };
                let result = service.handle(None, operation).unwrap();
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

        // No arguments: the bounded digest, not the state dump.
        let output = cli(&address, &["boss", "view"]);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let digest: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(digest["type"], "stateDigest");
        assert_eq!(digest["digest"]["counts"]["outcomes"], 1);
        assert!(digest["digest"]["counts"]["employees"].is_number());
        assert!(digest["digest"]["revision"].is_number());
        let sections = digest["sections"].as_array().unwrap();
        assert!(sections.contains(&serde_json::json!("outcomes")));
        assert!(
            output.stdout.len() < 4 * 1024,
            "digest must stay bounded: {} bytes",
            output.stdout.len()
        );

        // A named section returns just that collection.
        let output = cli(&address, &["boss", "view", "outcomes"]);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let slice: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        let outcomes = slice["outcomes"].as_array().unwrap();
        assert_eq!(outcomes.len(), 1);
        assert_eq!(outcomes[0]["outcome"], "compact view outcome");
        assert!(slice.get("employees").is_none());
        assert!(slice.get("personas").is_none());

        // Scalars live under `identity`.
        let output = cli(&address, &["boss", "view", "identity"]);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let slice: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert!(slice["identity"]["name"].is_string());
        assert!(slice["revision"].is_number());

        // --full preserves the complete state envelope.
        let output = cli(&address, &["boss", "view", "--full"]);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let full: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(full["type"], "state");
        assert_eq!(full["state"]["outcomes"].as_array().unwrap().len(), 1);
        assert!(full["state"]["employees"].is_array());

        // Bad combinations and unknown sections fail without a request-worthy shape.
        for args in [
            &["boss", "view", "bogus"] as &[_],
            &["boss", "view", "--full", "outcomes"],
            &["boss", "view", "outcomes", "employees"],
        ] {
            let output = cli(&address, args);
            assert!(
                !output.status.success(),
                "{args:?} must fail: {}",
                String::from_utf8_lossy(&output.stdout)
            );
        }
        let output = cli(&address, &["boss", "view", "bogus"]);
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("unknown section"),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );

        done.store(true, Ordering::Relaxed);
    });

    drop(service);
    std::fs::remove_dir_all(root).unwrap();
}
