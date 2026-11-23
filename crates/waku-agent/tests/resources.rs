#![cfg(unix)]
//! Exercise the real agent binary and shell workloads through a simulated daemon.
//! Scheduler correctness and cross-daemon persistence are tested in waku-core.
use std::{
    collections::VecDeque,
    net::TcpListener,
    process::{Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use uuid::Uuid;
use waku_protocol::{
    ClientMessage, Command as WireCommand, PROTOCOL_VERSION, ResponseOutcome, ResponsePayload,
    RpcError, ServerMessage, resources::*,
};

#[derive(Default)]
struct State {
    reservations: Vec<Reservation>,
    actions: Vec<String>,
    cancel: bool,
    deny_attach: bool,
    park_acquire: bool,
    status_errors: VecDeque<RpcError>,
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
                    task_views_available: false,
                    transcript_windows_available: false,
                    landing_queue_available: false,
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
                let mut status_error = None;
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
                            let granted_at = (!state.park_acquire).then_some(1);
                            state.reservations.push(Reservation {
                                id: next,
                                task: request.session_id,
                                resources,
                                purpose,
                                holder_pid,
                                daemon_pid: std::process::id(),
                                owner_pid: None,
                                workload_pid: None,
                                workload_started: None,
                                reap_signalled_at: None,
                                requested_at: 1,
                                duration_seconds: 0,
                                deadline: u64::from(wait_seconds),
                                granted_at,
                                cancelled: false,
                                released: false,
                                admission: None,
                                waiting_on: None,
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
                        status_error = state.status_errors.pop_front();
                        id = requested;
                        if state.park_acquire
                            && status_error.is_none()
                            && let Some(r) =
                                state.reservations.iter_mut().find(|r| Some(r.id) == id)
                        {
                            r.granted_at = Some(2);
                        }
                        if state.cancel {
                            for r in &mut state.reservations {
                                r.cancelled = true;
                            }
                        }
                    }
                    ResourceOperation::Admission { .. } | ResourceOperation::Reanchor { .. } => {
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
                        outcome: if let Some(error) = status_error {
                            ResponseOutcome::Error { error }
                        } else {
                            ResponseOutcome::Ok {
                                payload: ResponsePayload::AgentResources {
                                    status: ResourceStatus {
                                        reservations: state.reservations.clone(),
                                        request_id: id,
                                        borrowed,
                                        ..Default::default()
                                    },
                                },
                            }
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
            .env_remove("GODDARD_RESOURCE_RESERVATION")
            .env_remove(waku_protocol::RUNTIME_INGRESS_ENV);
        command
    }
}
impl Drop for Daemon {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        let _ = self.thread.take().unwrap().join();
    }
}

#[test]
fn acquire_keeps_polling_the_same_reservation_after_admission_busy() {
    let daemon = Daemon::start();
    {
        let mut state = daemon.state.lock().unwrap();
        state.park_acquire = true;
        state
            .status_errors
            .push_back(RpcError::admission_busy("test overload"));
    }
    let output = daemon
        .cli()
        .args([
            "resource",
            "acquire",
            "--output",
            "json",
            "--json",
            r#"{"resources":{"native_builds":1},"purpose":"queued test","wait_seconds":10}"#,
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let reply: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let state = daemon.state.lock().unwrap();
    assert_eq!(state.actions, ["acquire", "status", "status"]);
    assert_eq!(state.reservations.len(), 1);
    assert_eq!(reply["id"], state.reservations[0].id.to_string());
    assert_eq!(state.reservations[0].granted_at, Some(2));
}
const PAYLOAD: &str = r#"{"resources":{"native_builds":1},"purpose":"simulated native build"}"#;

#[test]
fn supervision_retries_only_admission_busy_without_losing_the_workload() {
    let busy = RpcError::admission_busy("daemon is busy; retry shortly");
    for (name, errors, child_seconds, succeeds) in [
        ("transient", vec![busy.clone()], 1, true),
        (
            "authentication",
            vec![RpcError::from(anyhow::anyhow!("authentication failed"))],
            10,
            false,
        ),
        ("persistent", vec![busy.clone(); 100], 10, false),
        ("completed_while_busy", vec![busy.clone(); 100], 1, true),
    ] {
        let daemon = Daemon::start();
        daemon.state.lock().unwrap().status_errors = errors.into();
        let script = format!("/bin/sleep {child_seconds}; printf survived");
        let began = Instant::now();
        let mut child = daemon
            .cli()
            .args([
                "resource", "run", "--json", PAYLOAD, "--", "/bin/sh", "-c", &script,
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        while child.try_wait().unwrap().is_none() {
            if began.elapsed() >= Duration::from_secs(12) {
                // The simulated workload ends on its own after at most ten
                // seconds, so this timeout cannot strand its process group.
                let _ = child.kill();
                let _ = child.wait();
                panic!("{name}: resource supervision did not settle");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let output = child.wait_with_output().unwrap();
        let elapsed = began.elapsed();
        assert_eq!(
            output.status.success(),
            succeeds,
            "{name}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(output.stdout == b"survived", succeeds, "{name}");
        let state = daemon.state.lock().unwrap();
        let statuses = state
            .actions
            .iter()
            .filter(|action| *action == "status")
            .count();
        for action in ["acquire", "attach", "release"] {
            assert_eq!(
                state.actions.iter().filter(|seen| *seen == action).count(),
                1,
                "{name}: {action} retried"
            );
        }
        assert!(state.reservations[0].released, "{name}");
        let pid = state.reservations[0].workload_pid.unwrap();
        assert!(running_group_members(pid).is_empty(), "{name}");
        match name {
            "authentication" => {
                assert_eq!(statuses, 1);
                assert!(elapsed < Duration::from_secs(2));
            }
            "persistent" => {
                assert!((2..=30).contains(&statuses));
                assert!(elapsed >= Duration::from_millis(4_500));
                assert!(elapsed < Duration::from_secs(8));
                assert!(String::from_utf8_lossy(&output.stderr).contains("busy"));
            }
            _ => {
                assert!(statuses >= 2);
                assert!(elapsed < Duration::from_secs(3));
            }
        }
        eprintln!("supervision-proof {name} elapsed={elapsed:?} status_attempts={statuses}");
    }
}

#[test]
fn authentication_failure_stops_descendants_created_during_shutdown() {
    use std::os::unix::fs::DirBuilderExt;
    let daemon = Daemon::start();
    let directory = std::env::temp_dir().join(format!("resource-shutdown-{}", Uuid::new_v4()));
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&directory)
        .unwrap();
    let ready = directory.join("ready");
    let descendant = directory.join("descendant");
    let script = r#"
trap '/bin/sleep 0.05; /bin/sleep 10 & printf "%s" "$!" > "$RESOURCE_TEST_DESCENDANT"; exit 0' TERM
printf ready > "$RESOURCE_TEST_READY"
/bin/sleep 10
printf completed
"#;
    let mut child = daemon
        .cli()
        .env("RESOURCE_TEST_READY", &ready)
        .env("RESOURCE_TEST_DESCENDANT", &descendant)
        .args([
            "resource", "run", "--json", PAYLOAD, "--", "/bin/sh", "-c", script,
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !ready.exists() {
        assert!(
            Instant::now() < deadline,
            "workload did not install its handler"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    let (group, statuses_before) = {
        let mut state = daemon.state.lock().unwrap();
        let statuses = state
            .actions
            .iter()
            .filter(|action| *action == "status")
            .count();
        state
            .status_errors
            .push_back(RpcError::from(anyhow::anyhow!("authentication failed")));
        (state.reservations[0].workload_pid.unwrap(), statuses)
    };
    let began = Instant::now();
    while child.try_wait().unwrap().is_none() {
        if began.elapsed() >= Duration::from_secs(5) {
            let _ = child.kill();
            let _ = child.wait();
            panic!("authentication failure did not stop supervision");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let descendant_record = match std::fs::read_to_string(&descendant) {
        Ok(record) => record,
        Err(error) => {
            eprintln!(
                "shutdown fixture did not create a child; group={:?}",
                running_group_members(group)
            );
            let output = child.wait_with_output().unwrap();
            std::fs::remove_dir_all(directory).unwrap();
            panic!(
                "termination handler did not record its child: {error}; {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
    };
    let descendant_pid: u32 = descendant_record.trim().parse().unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    while running_group_members(group).contains(&descendant_pid) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    let leaked = running_group_members(group).contains(&descendant_pid);
    if leaked {
        // SAFETY: the fixture recorded this child's PID and verifies its
        // owned group immediately before cleaning up the failed experiment.
        unsafe {
            if libc::getpgid(descendant_pid as i32) == group as i32 {
                libc::kill(descendant_pid as i32, libc::SIGKILL);
            }
        }
    }
    let output = child.wait_with_output().unwrap();
    std::fs::remove_dir_all(directory).unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("authentication failed"));
    let state = daemon.state.lock().unwrap();
    assert_eq!(
        state
            .actions
            .iter()
            .filter(|action| *action == "status")
            .count(),
        statuses_before + 1
    );
    assert!(state.reservations[0].released);
    eprintln!(
        "shutdown-descendant elapsed={:?} leaked={leaked}",
        began.elapsed()
    );
    assert!(
        !leaked,
        "supervisor exited while a child in its owned group kept running"
    );
}

fn running_group_members(group: u32) -> Vec<u32> {
    let output = Command::new("/bin/ps")
        .args(["-axo", "pid=,pgid=,stat="])
        .output()
        .unwrap();
    assert!(output.status.success());
    std::str::from_utf8(&output.stdout)
        .unwrap()
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let pid = fields.next()?.parse::<u32>().ok()?;
            let owner = fields.next()?.parse::<u32>().ok()?;
            let state = fields.next()?;
            (owner == group && !state.starts_with('Z')).then_some(pid)
        })
        .collect()
}

#[test]
fn run_preserves_standard_streams_exit_status_and_releases() {
    let daemon = Daemon::start();
    let mut child = daemon
        .cli()
        .args([
            "resource",
            "run",
            "--json",
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
            "--json",
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
            "--json",
            PAYLOAD,
            "--",
            "/bin/sh",
            "-c",
            "\"$TEST_CLI\" resource run --json \"$TEST_PAYLOAD\" -- /bin/sh -c 'printf nested'",
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
        .args([
            "resource",
            "run",
            "--json",
            PAYLOAD,
            "--",
            "/bin/sleep",
            "30",
        ])
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
