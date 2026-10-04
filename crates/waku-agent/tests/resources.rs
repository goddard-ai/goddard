#![cfg(unix)]
//! Exercise the real agent binary and shell workloads through a simulated daemon.
//! Scheduler correctness and cross-daemon persistence are tested in waku-core.
use std::{
    net::TcpListener,
    process::{Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use uuid::Uuid;
use waku_protocol::{
    ClientMessage, Command as WireCommand, PROTOCOL_VERSION, ResponseOutcome, ResponsePayload,
    ServerMessage, resources::*,
};

#[derive(Default)]
struct State {
    reservations: Vec<Reservation>,
    actions: Vec<String>,
    cancel: bool,
    deny_attach: bool,
}
struct Daemon {
    address: String,
    state: Arc<Mutex<State>>,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Daemon {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap().to_string();
        let stop = Arc::new(AtomicBool::new(false));
        let shutdown = stop.clone();
        let state = Arc::new(Mutex::new(State::default()));
        let shared = state.clone();
        let thread = std::thread::spawn(move || {
            while !shutdown.load(Ordering::Acquire) {
                let stream = match listener.accept() {
                    Ok((stream, _)) => stream,
                    Err(_) => {
                        std::thread::sleep(Duration::from_millis(5));
                        continue;
                    }
                };
                // Accepted sockets inherit O_NONBLOCK from the listener on macOS.
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let Ok(mut socket) = tungstenite::accept(stream) else {
                    continue;
                };
                let hello = socket.read().unwrap();
                let hello: ClientMessage = serde_json::from_str(hello.to_text().unwrap()).unwrap();
                assert!(
                    matches!(hello, ClientMessage::Hello { token, .. } if token == "test-task-token")
                );
                let message = ServerMessage::Hello {
                    protocol_version: PROTOCOL_VERSION,
                    daemon_version: "test".into(),
                    daemon_commit: None,
                    agent_cli_available: true,
                };
                socket
                    .send(tungstenite::Message::Text(
                        serde_json::to_string(&message).unwrap().into(),
                    ))
                    .unwrap();
                let request = socket.read().unwrap();
                let request: ClientMessage =
                    serde_json::from_str(request.to_text().unwrap()).unwrap();
                let ClientMessage::Request(request) = request else {
                    panic!("expected request")
                };
                let WireCommand::AgentResources { operation } = request.command else {
                    panic!("expected resource command")
                };
                let mut state = shared.lock().unwrap();
                let id;
                let mut borrowed = false;
                let mut refused = false;
                match operation {
                    ResourceOperation::Acquire {
                        resources,
                        purpose,
                        holder_pid,
                        wait_seconds,
                        parent,
                    } => {
                        state.actions.push("acquire".into());
                        if let Some(parent) = parent {
                            id = Some(parent);
                            borrowed = true;
                        } else {
                            let next = Uuid::new_v4();
                            id = Some(next);
                            state.reservations.push(Reservation {
                                id: next,
                                task: request.session_id,
                                resources,
                                purpose,
                                holder_pid,
                                daemon_pid: std::process::id(),
                                workload_pid: None,
                                requested_at: 1,
                                duration_seconds: 0,
                                deadline: u64::from(wait_seconds),
                                granted_at: Some(1),
                                cancelled: false,
                                released: false,
                                admission: None,
                            });
                        }
                    }
                    ResourceOperation::Attach {
                        id: attach_id,
                        workload_pid,
                    } => {
                        state.actions.push("attach".into());
                        id = Some(attach_id);
                        refused = state.deny_attach;
                        if !refused {
                            state
                                .reservations
                                .iter_mut()
                                .find(|r| r.id == attach_id)
                                .unwrap()
                                .workload_pid = Some(workload_pid);
                        }
                    }
                    ResourceOperation::Release { id: release }
                    | ResourceOperation::Cancel { id: release } => {
                        state.actions.push("release".into());
                        id = Some(release);
                        let r = state
                            .reservations
                            .iter_mut()
                            .find(|r| r.id == release)
                            .unwrap();
                        r.released = true;
                        r.cancelled = true;
                    }
                    ResourceOperation::Status { id: requested } => {
                        state.actions.push("status".into());
                        id = requested;
                        if state.cancel {
                            for r in &mut state.reservations {
                                r.cancelled = true;
                            }
                        }
                    }
                    ResourceOperation::Admission { .. } => {
                        unreachable!("admission reservations are daemon-internal")
                    }
                }
                let message = if refused {
                    // Close transport before allowing the gated command to run.
                    drop(state);
                    let _ = socket.close(None);
                    continue;
                } else {
                    ServerMessage::Response {
                        request_id: request.request_id,
                        outcome: ResponseOutcome::Ok {
                            payload: ResponsePayload::AgentResources {
                                status: ResourceStatus {
                                    reservations: state.reservations.clone(),
                                    request_id: id,
                                    borrowed,
                                    ..Default::default()
                                },
                            },
                        },
                    }
                };
                drop(state);
                let _ = socket.send(tungstenite::Message::Text(
                    serde_json::to_string(&message).unwrap().into(),
                ));
            }
        });
        Self {
            address,
            state,
            stop,
            thread: Some(thread),
        }
    }
    fn cli(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_goddard-agent"));
        command
            .env("GODDARD_DAEMON_ADDRESS", &self.address)
            .env("GODDARD_AGENT_TOKEN", "test-task-token")
            .env("GODDARD_TASK_ID", Uuid::new_v4().to_string())
            .env_remove("GODDARD_RESOURCE_RESERVATION");
        command
    }
}
impl Drop for Daemon {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        let _ = self.thread.take().unwrap().join();
    }
}
const PAYLOAD: &str = r#"{"resources":{"native_builds":1},"purpose":"simulated native build"}"#;

#[test]
fn run_preserves_standard_streams_exit_status_and_releases() {
    let daemon = Daemon::start();
    let mut child = daemon
        .cli()
        .args([
            "resource",
            "run",
            PAYLOAD,
            "--",
            "/bin/sh",
            "-c",
            "read value; printf 'out:%s' \"$value\"; printf 'err' >&2; exit 7",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    use std::io::Write;
    child.stdin.take().unwrap().write_all(b"hello\n").unwrap();
    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(7));
    assert_eq!(output.stdout, b"out:hello");
    assert_eq!(output.stderr, b"err");
    let state = daemon.state.lock().unwrap();
    assert_eq!(&state.actions[..2], &["acquire", "attach"]);
    assert!(state.reservations[0].released);
}

#[test]
fn refused_registration_never_starts_the_workload() {
    let daemon = Daemon::start();
    daemon.state.lock().unwrap().deny_attach = true;
    let marker = std::env::temp_dir().join(format!("goddard-resource-marker-{}", Uuid::new_v4()));
    let output = daemon
        .cli()
        .env("TEST_MARKER", &marker)
        .args([
            "resource",
            "run",
            PAYLOAD,
            "--",
            "/bin/sh",
            "-c",
            "touch \"$TEST_MARKER\"",
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(!marker.exists());
}

#[test]
fn nested_run_borrows_parent_and_only_outer_runner_attaches_and_releases() {
    let daemon = Daemon::start();
    let output = daemon
        .cli()
        .env("TEST_CLI", env!("CARGO_BIN_EXE_goddard-agent"))
        .env("TEST_PAYLOAD", PAYLOAD)
        .args([
            "resource",
            "run",
            PAYLOAD,
            "--",
            "/bin/sh",
            "-c",
            "\"$TEST_CLI\" resource run \"$TEST_PAYLOAD\" -- /bin/sh -c 'printf nested'",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"nested");
    let state = daemon.state.lock().unwrap();
    assert_eq!(state.actions.iter().filter(|s| *s == "acquire").count(), 2);
    assert_eq!(state.actions.iter().filter(|s| *s == "attach").count(), 1);
    assert_eq!(state.actions.iter().filter(|s| *s == "release").count(), 1);
}

#[test]
fn cancellation_terminates_only_the_owned_simulated_process_group() {
    let daemon = Daemon::start();
    let child = daemon
        .cli()
        .args(["resource", "run", PAYLOAD, "--", "/bin/sleep", "30"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    let pid = loop {
        if let Some(pid) = daemon
            .state
            .lock()
            .unwrap()
            .reservations
            .first()
            .and_then(|r| r.workload_pid)
        {
            break pid;
        }
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    };
    daemon.state.lock().unwrap().cancel = true;
    let output = child.wait_with_output().unwrap();
    assert!(!output.status.success());
    assert_eq!(unsafe { libc::kill(-(pid as i32), 0) }, -1);
    assert!(daemon.state.lock().unwrap().reservations[0].released);
}
