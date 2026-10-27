//! Owned ACP stdio lifetime. The SDK still owns protocol framing and routing;
//! this adapter owns shutdown, reaping, and reader completion.

use super::*;
use std::io::{BufRead as _, BufReader, Write as _};
use std::sync::mpsc::{RecvTimeoutError, SyncSender};
use std::time::Instant;

#[derive(Clone)]
pub(super) struct Control {
    stop: SyncSender<()>,
    requested: Arc<AtomicBool>,
}

impl Control {
    pub(super) fn expect_exit(&self) {
        self.requested.store(true, Ordering::Release);
    }

    pub(super) fn begin_shutdown(&self) {
        self.expect_exit();
        self.request_stop();
    }

    fn request_stop(&self) {
        let _ = self.stop.try_send(());
    }

    /// The SDK may cancel its transport future when the session handler
    /// returns. The owning actor must wait outside that cancellable future.
    pub(super) fn finish(
        &self,
        result: agent_client_protocol::Result<()>,
        exited: Completion,
    ) -> agent_client_protocol::Result<()> {
        self.request_stop();
        let status = exited
            .recv_blocking()
            .map_err(agent_client_protocol::Error::into_internal_error)
            .and_then(|status| status.map_err(agent_client_protocol::Error::into_internal_error));
        result?;
        let status = status?;
        if !self.requested.load(Ordering::Acquire) && !status.success() {
            return Err(agent_client_protocol::Error::into_internal_error(
                std::io::Error::other(format!("ACP provider exited with {status}")),
            ));
        }
        Ok(())
    }
}

pub(super) type Completion = smol::channel::Receiver<std::io::Result<std::process::ExitStatus>>;

struct StopOnDrop(Control);

impl Drop for StopOnDrop {
    fn drop(&mut self) {
        self.0.request_stop();
    }
}

type Lines = agent_client_protocol::Lines<
    std::pin::Pin<Box<dyn futures::Sink<String, Error = std::io::Error> + Send>>,
    std::pin::Pin<Box<dyn futures::Stream<Item = std::io::Result<String>> + Send>>,
>;

pub(super) struct Transport {
    lines: Lines,
    stop: StopOnDrop,
}

impl ConnectTo<Client> for Transport {
    async fn connect_to(self, client: impl ConnectTo<Agent>) -> agent_client_protocol::Result<()> {
        let Self { lines, stop: _stop } = self;
        ConnectTo::<Client>::connect_to(lines, client).await
    }
}

pub(super) fn spawn(
    command: &std::process::Command,
    sandbox: Option<&Arc<crate::sandbox::ShuruVm>>,
    stderr_lines: Arc<Mutex<Vec<String>>>,
    events: &DriverEventSender,
    stopped: smol::channel::Sender<CommandMessage>,
) -> anyhow::Result<(Transport, Control, Completion)> {
    let mut child = crate::sandbox::spawn(command, sandbox)?;
    events.record_process(child.host_pid())?;
    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| anyhow!("ACP stdin unavailable"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow!("ACP stdout unavailable"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| anyhow!("ACP stderr unavailable"))?;
    let stderr_thread = thread::Builder::new()
        .name("waku-acp-stderr".into())
        .spawn(move || drain_stderr(stderr, &stderr_lines))?;
    // A stopped/slow protocol consumer must not accumulate unbounded output.
    let (mut lines_tx, lines_rx) = futures::channel::mpsc::channel(128);
    let stdout_thread = thread::Builder::new()
        .name("waku-acp-stdout".into())
        .spawn(move || {
            use futures::SinkExt as _;
            for line in BufReader::new(stdout).lines() {
                let failed = line.is_err();
                if futures::executor::block_on(lines_tx.send(line)).is_err() || failed {
                    break;
                }
            }
        })?;
    let (stop, shutdown) = std::sync::mpsc::sync_channel(1);
    let control = Control {
        stop,
        requested: Arc::new(AtomicBool::new(false)),
    };
    let (exit_tx, exited) = smol::channel::bounded(1);
    let events = events.clone();
    thread::Builder::new()
        .name("waku-acp-process".into())
        .spawn(move || {
            let mut stopping = None;
            let mut terminated = false;
            let status = loop {
                match child.try_wait() {
                    Ok(Some(status)) => break Ok(status),
                    Err(error) => {
                        // Best-effort cleanup cannot repair failed evidence.
                        let _ = child.kill();
                        let _ = child.wait();
                        break Err(error);
                    }
                    Ok(None) => {}
                }
                if let Some(since) = stopping {
                    let elapsed = Instant::now().saturating_duration_since(since);
                    if elapsed >= Duration::from_millis(1500) {
                        let _ = child.kill();
                        break child.wait();
                    }
                    if elapsed >= Duration::from_secs(1) && !terminated {
                        let _ = child.terminate();
                        terminated = true;
                    }
                }
                match shutdown.recv_timeout(Duration::from_millis(100)) {
                    Ok(()) | Err(RecvTimeoutError::Disconnected) => {
                        stopping.get_or_insert_with(Instant::now);
                        // A disconnected channel stays ready. Do not spin
                        // while the child uses its remaining shutdown grace.
                        if matches!(
                            shutdown.try_recv(),
                            Err(std::sync::mpsc::TryRecvError::Disconnected)
                        ) {
                            thread::sleep(Duration::from_millis(100));
                        }
                    }
                    Err(RecvTimeoutError::Timeout) => {}
                }
            };
            let stdout_stopped = stdout_thread.join().is_ok();
            let stderr_result = stderr_thread.join();
            let stderr_stopped = matches!(stderr_result, Ok(Ok(())));
            let status = if stdout_stopped && stderr_stopped {
                status
            } else {
                Err(std::io::Error::other(
                    "ACP reader drain did not complete successfully",
                ))
            };
            if status.is_ok() {
                events.confirm_writer_stopped();
            }
            let _ = exit_tx.send_blocking(status);
            // Plain stream EOF can leave the SDK awaiting its session
            // handler. Wake that handler even after an unrequested exit.
            let _ = stopped.try_send(CommandMessage::ProcessStopped);
        })?;
    let incoming: std::pin::Pin<Box<dyn futures::Stream<Item = std::io::Result<String>> + Send>> =
        Box::pin(lines_rx);
    let outgoing: std::pin::Pin<Box<dyn futures::Sink<String, Error = std::io::Error> + Send>> =
        Box::pin(futures::sink::unfold(
            stdin,
            |mut writer, line: String| async move {
                writer.write_all(line.as_bytes())?;
                writer.write_all(b"\n")?;
                Ok::<_, std::io::Error>(writer)
            },
        ));
    Ok((
        Transport {
            lines: agent_client_protocol::Lines::new(outgoing, incoming),
            stop: StopOnDrop(control.clone()),
        },
        control,
        exited,
    ))
}

// Match the SDK's bounded diagnostic tail even for a provider that never
// writes a newline. BufRead::lines would allocate the entire diagnostic first.
fn drain_stderr(mut stderr: impl std::io::Read, lines: &Mutex<Vec<String>>) -> std::io::Result<()> {
    const LINE_BYTES: usize = 8192;
    const TAIL_BYTES: usize = 64 * 1024;
    let mut current = Vec::with_capacity(LINE_BYTES);
    let mut buffer = [0; 8192];
    let record = |bytes: &[u8]| {
        let line = String::from_utf8_lossy(bytes).into_owned();
        if line.trim().is_empty() {
            return;
        }
        let mut lines = lines.lock();
        let mut size: usize = lines.iter().map(String::len).sum();
        while !lines.is_empty() && (lines.len() >= 128 || size + line.len() > TAIL_BYTES) {
            size -= lines.remove(0).len();
        }
        lines.push(line);
    };
    loop {
        let read = match stderr.read(&mut buffer) {
            Ok(0) => {
                record(&current);
                return Ok(());
            }
            Ok(read) => read,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => {
                record(&current);
                return Err(error);
            }
        };
        for part in buffer[..read].split_inclusive(|byte| *byte == b'\n') {
            let excess = (current.len() + part.len()).saturating_sub(LINE_BYTES);
            current.drain(..excess);
            current.extend_from_slice(part);
            if part.last() == Some(&b'\n') {
                record(&current);
                current.clear();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stderr_tail_is_bounded_even_without_newlines() {
        let lines = Mutex::new(Vec::new());
        let mut diagnostic = vec![b'x'; 2 * 1024 * 1024];
        diagnostic.extend_from_slice(b"last diagnostic\n");
        drain_stderr(diagnostic.as_slice(), &lines).unwrap();
        assert!(lines.lock().last().unwrap().ends_with("last diagnostic\n"));
        assert!(lines.lock().iter().map(String::len).sum::<usize>() <= 64 * 1024);
        // Also bound the total across many independently bounded lines.
        let many_lines = [vec![b'y'; 4096], vec![b'\n']].concat().repeat(100);
        drain_stderr(many_lines.as_slice(), &lines).unwrap();
        assert!(lines.lock().iter().map(String::len).sum::<usize>() <= 64 * 1024);
    }

    #[cfg(unix)]
    fn start(
        binary: &Path,
        cwd: &Path,
    ) -> (
        crate::driver::DriverHandle,
        crossbeam_channel::Receiver<DriverEvent>,
    ) {
        let (events, received) = crate::driver::test_event_channel();
        let driver = crate::driver::start_local(
            ProviderKind::Devin,
            DriverStartOptions {
                binary: binary.to_owned(),
                cwd: cwd.to_owned(),
                mode: RuntimeMode::Ask,
                model: None,
                reasoning_effort: None,
                service_tier: None,
                context_window: None,
                agent_preset: None,
                computer_use_enabled: false,
                computer_use_runtime: None,
                agent: None,
                read_own_transcript: false,
                subagents: None,
                mcp_servers: Vec::new(),
                http_mcp_capability_recorder: None,
                provider_cursor: None,
                eval: None,
                sandbox: None,
                allow_model_fallback: false,
                ephemeral: false,
            },
            events,
        )
        .unwrap();
        (driver, received)
    }

    #[cfg(unix)]
    fn peer(root: &Path) -> std::path::PathBuf {
        use std::os::unix::fs::PermissionsExt as _;
        let binary = root.join("provider");
        std::fs::write(&binary, r#"#!/usr/bin/env python3
import json, pathlib, sys, time
for line in sys.stdin:
    request = json.loads(line)
    if 'id' not in request:
        continue
    method = request.get('method')
    if method == 'initialize':
        result = {'protocolVersion': 1, 'agentCapabilities': {}, 'agentInfo': {'name': 'fixture', 'version': '1'}}
    elif method == 'session/new':
        result = {'sessionId': 'fixture-session'}
    else:
        result = {}
    print(json.dumps({'jsonrpc': '2.0', 'id': request['id'], 'result': result}), flush=True)
    if method == 'session/new' and pathlib.Path('fail').exists():
        while not pathlib.Path('release').exists():
            time.sleep(0.005)
        code = int(pathlib.Path('fail').read_text())
        if code:
            sys.stderr.write('x' * (2 * 1024 * 1024) + '\nError: controlled ACP failure\n')
            sys.stderr.flush()
        raise SystemExit(code)
pathlib.Path('stdin-closed').touch()
while not pathlib.Path('release').exists():
    time.sleep(0.005)
pathlib.Path('final-edit').write_text('preserve this after shutdown')
"#).unwrap();
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
        binary
    }

    #[cfg(unix)]
    #[test]
    fn real_acp_shutdown_confirms_the_child_after_its_final_workspace_write() {
        use crate::driver::WriterCompletionState;
        let root = std::env::temp_dir().join(format!("acp-owned-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let binary = peer(&root);
        // Let the controlled process finish even if an assertion unwinds.
        struct Release(std::path::PathBuf);
        impl Drop for Release {
            fn drop(&mut self) {
                let _ = std::fs::write(self.0.join("release"), "");
            }
        }
        let _release = Release(root.clone());
        let (driver, received) = start(&binary, &root);
        let witness = driver.writer_completion();
        assert!(matches!(
            received.recv_timeout(Duration::from_secs(5)).unwrap(),
            DriverEvent::Connected { .. }
        ));
        let retained = driver.clone();
        driver.begin_shutdown();
        drop(driver);
        let deadline = Instant::now() + Duration::from_secs(5);
        while !root.join("stdin-closed").exists() {
            assert!(
                Instant::now() < deadline,
                "ACP did not close its owned stdin"
            );
            thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(witness.state(), WriterCompletionState::Pending);
        assert!(!root.join("final-edit").exists());
        std::fs::write(root.join("release"), "").unwrap();
        loop {
            match received
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .unwrap()
            {
                DriverEvent::ProcessExited => break,
                DriverEvent::Error(error) => panic!("controlled ACP failed: {error}"),
                _ => {}
            }
        }
        assert_eq!(witness.state(), WriterCompletionState::Confirmed);
        assert_eq!(
            std::fs::read_to_string(root.join("final-edit")).unwrap(),
            "preserve this after shutdown"
        );
        drop(retained);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn natural_acp_exits_wake_the_actor_preserve_diagnostics_and_confirm_retirement() {
        for code in [0, 42] {
            let root = std::env::temp_dir().join(format!("acp-failed-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&root).unwrap();
            std::fs::write(root.join("fail"), code.to_string()).unwrap();
            let binary = peer(&root);
            let (driver, received) = start(&binary, &root);
            assert!(matches!(
                received.recv_timeout(Duration::from_secs(5)).unwrap(),
                DriverEvent::Connected { .. }
            ));
            std::fs::write(root.join("release"), "").unwrap();
            let deadline = Instant::now() + Duration::from_secs(5);
            let mut diagnostic = None;
            loop {
                match received
                    .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                    .unwrap()
                {
                    DriverEvent::Error(error) => diagnostic = Some(error),
                    DriverEvent::ProcessExited => break,
                    _ => {}
                }
            }
            if code == 0 {
                assert!(
                    diagnostic.is_none(),
                    "clean exit became an error: {diagnostic:?}"
                );
            } else {
                assert!(diagnostic.unwrap().contains("controlled ACP failure"));
            }
            assert_eq!(
                driver.writer_completion().state(),
                crate::driver::WriterCompletionState::Confirmed
            );
            drop(driver);
            std::fs::remove_dir_all(root).unwrap();
        }
    }

    #[cfg(unix)]
    #[test]
    fn shutdown_reaps_an_acp_child_that_never_finishes_initialization() {
        use std::os::unix::fs::PermissionsExt as _;
        let root = std::env::temp_dir().join(format!("acp-stalled-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let binary = root.join("provider");
        std::fs::write(
            &binary,
            r#"#!/usr/bin/env python3
import pathlib, signal, sys, time
signal.signal(signal.SIGTERM, signal.SIG_IGN)
pathlib.Path('ready').touch()
for line in sys.stdin:
    pass
while True:
    time.sleep(1)
"#,
        )
        .unwrap();
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
        let (driver, received) = start(&binary, &root);
        let witness = driver.writer_completion();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !root.join("ready").exists() {
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(5));
        }
        driver.begin_shutdown();
        loop {
            if matches!(
                received
                    .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                    .unwrap(),
                DriverEvent::ProcessExited
            ) {
                break;
            }
        }
        assert_eq!(
            witness.state(),
            crate::driver::WriterCompletionState::Confirmed
        );
        drop(driver);
        std::fs::remove_dir_all(root).unwrap();
    }
}
