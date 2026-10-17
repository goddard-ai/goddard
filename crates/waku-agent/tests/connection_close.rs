//! The CLI must finish its WebSocket close before its process exits.
use std::{
    io::Read as _,
    net::TcpListener,
    process::Command,
    time::{Duration, Instant},
};

use tungstenite::Message;
use waku_protocol::{
    ClientMessage, Command as WireCommand, PROTOCOL_VERSION, ResponseOutcome, ResponsePayload,
    RpcError, ServerMessage,
};

#[test]
fn cli_closes_cleanly_after_success_and_failure_without_waiting_for_a_stalled_peer() {
    for (success, reply_to_close) in [(true, true), (false, true), (true, false)] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap().to_string();
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut socket = tungstenite::accept(stream).unwrap();
            let hello = socket.read().unwrap();
            let hello: ClientMessage = serde_json::from_str(hello.to_text().unwrap()).unwrap();
            assert!(matches!(hello, ClientMessage::Hello { .. }));
            socket
                .send(Message::Text(
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
            let request = socket.read().unwrap();
            let request: ClientMessage = serde_json::from_str(request.to_text().unwrap()).unwrap();
            let ClientMessage::Request(request) = request else {
                panic!("expected request");
            };
            assert!(matches!(request.command, WireCommand::AgentListModels));
            let outcome = if success {
                ResponseOutcome::Ok {
                    payload: ResponsePayload::AgentModelOptions { options: vec![] },
                }
            } else {
                ResponseOutcome::Error {
                    error: RpcError {
                        message: "diagnostic RPC failure".into(),
                        i18n: None,
                    },
                }
            };
            socket
                .send(Message::Text(
                    serde_json::to_string(&ServerMessage::Response {
                        request_id: request.request_id,
                        outcome,
                    })
                    .unwrap()
                    .into(),
                ))
                .unwrap();
            let closing = socket.read();
            assert!(
                matches!(closing, Ok(Message::Close(_))),
                "CLI exited without a WebSocket Close frame: {closing:?}"
            );
            if reply_to_close {
                // Server-side completion can return ConnectionClosed
                // after successfully writing the automatic Close reply.
                let flushed = socket.flush();
                assert!(
                    matches!(flushed, Ok(()) | Err(tungstenite::Error::ConnectionClosed)),
                    "could not complete closing handshake: {flushed:?}"
                );
            } else {
                // Do not let tungstenite flush its automatic Close reply:
                // the CLI must abandon an unresponsive peer within budget.
                let started = Instant::now();
                assert_eq!(socket.get_mut().read(&mut [0]).unwrap(), 0);
                assert!(started.elapsed() < Duration::from_secs(2));
            }
        });
        let output = Command::new(env!("CARGO_BIN_EXE_goddard-agent"))
            .arg("models")
            .env(waku_protocol::DAEMON_ADDRESS_ENV, address)
            .env(waku_protocol::AGENT_TOKEN_ENV, "test-task-token")
            .env_remove(waku_protocol::RUNTIME_INGRESS_ENV)
            .output()
            .unwrap();
        assert_eq!(
            output.status.success(),
            success,
            "unexpected CLI status: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        server.join().unwrap();
    }
}
