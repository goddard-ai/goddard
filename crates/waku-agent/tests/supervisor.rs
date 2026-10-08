//! Exercise the employee command through the real CLI and scoped transport.
use std::{net::TcpListener, process::Command, time::Duration};
use uuid::Uuid;
use waku_protocol::{
    AgentPromptDelivery, ClientMessage, Command as WireCommand, PROTOCOL_VERSION, ResponseOutcome,
    ResponsePayload, ServerMessage,
};

#[test]
fn steer_supervisor_sends_an_unaddressed_prompt_with_the_employee_credential() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap().to_string();
    let employee = Uuid::new_v4();
    let prompt = "Progress: checks passed.\n$literal `tick`";
    let daemon = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut socket = tungstenite::accept(stream).unwrap();
        let hello: ClientMessage =
            serde_json::from_str(socket.read().unwrap().to_text().unwrap()).unwrap();
        assert!(matches!(hello, ClientMessage::Hello { token, .. } if token == "test-task-token"));
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
        let request: ClientMessage =
            serde_json::from_str(socket.read().unwrap().to_text().unwrap()).unwrap();
        let ClientMessage::Request(request) = request else {
            panic!("expected request")
        };
        assert_eq!(request.session_id, employee);
        assert!(matches!(request.command, WireCommand::AgentPrompt {
            task_id: None, thread_id: None, provider: None,
            prompt: ref text, delivery: AgentPromptDelivery::Interrupt,
        } if text == prompt));
        socket
            .send(tungstenite::Message::Text(
                serde_json::to_string(&ServerMessage::Response {
                    request_id: request.request_id,
                    outcome: ResponseOutcome::Ok {
                        payload: ResponsePayload::Ack,
                    },
                })
                .unwrap()
                .into(),
            ))
            .unwrap();
    });
    let output = Command::new(env!("CARGO_BIN_EXE_goddard-agent"))
        .args(["steer-supervisor", "--text", prompt])
        .env("GODDARD_DAEMON_ADDRESS", address)
        .env("GODDARD_AGENT_TOKEN", "test-task-token")
        .env("GODDARD_TASK_ID", employee.to_string())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap()["ok"],
        true
    );
    daemon.join().unwrap();

    // The dedicated command has no way to override routing or park a message.
    for extra in [
        vec!["--delivery", "queue"],
        vec!["00000000-0000-0000-0000-000000000000"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_goddard-agent"))
            .args(["steer-supervisor", "--text", "update"])
            .args(extra)
            .env_remove("GODDARD_DAEMON_ADDRESS")
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(!String::from_utf8_lossy(&output.stderr).contains("GODDARD_DAEMON_ADDRESS"));
    }
}
