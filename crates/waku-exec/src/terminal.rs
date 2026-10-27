//! Daemon-owned pseudoterminals for remote clients.
//!
//! The daemon owns the shell, cwd, and PTY. Clients only render the byte
//! stream and send input/resize controls, so a browser can operate against a
//! daemon on another machine without interpreting any daemon-side paths.

/// Receives live terminal events without retaining them in a replay journal.
pub trait TerminalEventSink: Send + 'static {
    /// Called after validation and before a PTY child can exist. Owners can
    /// establish durable writer evidence here; failure aborts the launch.
    fn before_spawn(&self, _cwd: &std::path::Path) -> anyhow::Result<()> {
        Ok(())
    }

    fn send_ephemeral(&self, event: waku_protocol::WireDriverEvent) -> anyhow::Result<()>;
}

#[cfg(not(unix))]
use std::path::Path;

#[cfg(not(unix))]
use anyhow::bail;

#[cfg(unix)]
mod platform {
    use std::io::{Read as _, Write as _};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::thread::JoinHandle;
    use std::time::{Duration, Instant};

    use alacritty_terminal::event::{OnResize as _, WindowSize};
    use alacritty_terminal::tty::{self, EventedPty as _, EventedReadWrite as _, Shell};
    use anyhow::{Context as _, bail};
    use base64::Engine as _;
    use parking_lot::Mutex;
    use serde_json::json;

    use super::TerminalEventSink;
    use waku_protocol::WireDriverEvent;

    const CELL_WIDTH: u16 = 8;
    const CELL_HEIGHT: u16 = 16;
    const MIN_COLUMNS: u16 = 2;
    const MIN_ROWS: u16 = 1;
    const SHELL_SHUTDOWN_GRACE: Duration = Duration::from_millis(250);
    /// Recent output kept for a reconnecting client to repaint from. Bounded
    /// so a chatty PTY cannot pin unbounded transcript bytes in daemon
    /// memory — the same reason output events are ephemeral, not journaled.
    const OUTPUT_TAIL_BYTES: usize = 256 * 1024;

    pub struct DaemonTerminal {
        pty: Arc<Mutex<tty::Pty>>,
        stopped: Arc<AtomicBool>,
        reader: Option<JoinHandle<()>>,
        tail: Arc<Mutex<Vec<u8>>>,
    }

    impl DaemonTerminal {
        pub fn open(
            cwd: &std::path::Path,
            cols: u16,
            rows: u16,
            events: impl TerminalEventSink,
        ) -> anyhow::Result<Self> {
            let shell = crate::command_env::default_terminal_shell();
            let shell_args = crate::command_env::default_terminal_shell_args(&shell);
            Self::open_with_shell(
                cwd,
                cols,
                rows,
                events,
                Shell::new(shell.to_string_lossy().into_owned(), shell_args),
            )
        }

        /// Prepare and source a command on this host, retaining the shell
        /// for inspection unless the command requests a successful close.
        pub fn open_command(
            cwd: &std::path::Path,
            cols: u16,
            rows: u16,
            events: impl TerminalEventSink,
            command: &waku_protocol::custom_commands::CustomCommand,
            scripts_directory: &std::path::Path,
        ) -> anyhow::Result<Self> {
            let shell = crate::terminal_command::command_shell(command);
            let script_path =
                crate::terminal_command::ensure_script(scripts_directory, &command.script)
                    .context("materialize custom command on the terminal host")?;
            let line = waku_protocol::terminal_command::source_line(
                &shell,
                &script_path,
                command.close_on_success,
            );
            let shell_args = crate::command_env::default_terminal_shell_args(&shell);
            let terminal = Self::open_with_shell(
                cwd,
                cols,
                rows,
                events,
                Shell::new(shell.to_string_lossy().into_owned(), shell_args),
            )?;
            terminal.write(format!("{line}\n").into_bytes())?;
            Ok(terminal)
        }

        pub fn open_with_shell(
            cwd: &std::path::Path,
            cols: u16,
            rows: u16,
            events: impl TerminalEventSink,
            shell: Shell,
        ) -> anyhow::Result<Self> {
            if !cwd.is_dir() {
                bail!(
                    "terminal working directory does not exist: {}",
                    cwd.display()
                );
            }

            // Use the observed physical path for the factory as well; an
            // alias retargeted by another operation must not redirect spawn
            // away from the directory whose ownership was recorded.
            let cwd = std::fs::canonicalize(cwd)
                .with_context(|| format!("resolve terminal working directory {}", cwd.display()))?;
            let mut options = tty::Options {
                shell: Some(shell),
                working_directory: Some(cwd.clone()),
                drain_on_exit: false,
                ..Default::default()
            };
            for (name, value) in crate::command_env::shell_environment() {
                options.env.insert(
                    name.to_string_lossy().into_owned(),
                    value.to_string_lossy().into_owned(),
                );
            }
            options.env.insert("TERM".into(), "xterm-256color".into());
            options.env.insert("COLORTERM".into(), "truecolor".into());

            let size = window_size(cols, rows);
            events.before_spawn(&cwd)?;
            let pty = tty::new(&options, size, 0)
                .with_context(|| format!("spawn terminal in {}", cwd.display()))?;
            let mut output = pty.file().try_clone().context("clone terminal output")?;
            let pty = Arc::new(Mutex::new(pty));
            let stopped = Arc::new(AtomicBool::new(false));
            let tail = Arc::new(Mutex::new(Vec::new()));
            let reader_pty = pty.clone();
            let reader_stopped = stopped.clone();
            let reader_tail = tail.clone();
            let reader = std::thread::Builder::new()
                .name("goddard-daemon-terminal-output".into())
                .spawn(move || {
                    let mut buffer = [0_u8; 32 * 1024];
                    while !reader_stopped.load(Ordering::Acquire) {
                        match output.read(&mut buffer) {
                            Ok(0) => {
                                let _ = events.send_ephemeral(WireDriverEvent::new(
                                    "terminalExited",
                                    serde_json::Value::Null,
                                ));
                                break;
                            }
                            Ok(read) => {
                                let data = base64::engine::general_purpose::STANDARD
                                    .encode(&buffer[..read]);
                                {
                                    let mut tail = reader_tail.lock();
                                    tail.extend_from_slice(&buffer[..read]);
                                    let excess = tail.len().saturating_sub(OUTPUT_TAIL_BYTES);
                                    tail.drain(..excess);
                                }
                                let _ = events.send_ephemeral(WireDriverEvent::new(
                                    "terminalOutput",
                                    json!({ "data": data }),
                                ));
                            }
                            Err(error)
                                if matches!(
                                    error.kind(),
                                    std::io::ErrorKind::WouldBlock
                                        | std::io::ErrorKind::TimedOut
                                        | std::io::ErrorKind::Interrupted
                                ) =>
                            {
                                std::thread::sleep(Duration::from_millis(4));
                            }
                            Err(error) if error.raw_os_error() == Some(libc::EIO) => {
                                // A PTY master may report EIO briefly before the
                                // freshly spawned child has attached its slave.
                                // Only treat it as EOF after Alacritty's SIGCHLD
                                // channel confirms the child actually exited.
                                if reader_pty.lock().next_child_event().is_some() {
                                    let _ = events.send_ephemeral(WireDriverEvent::new(
                                        "terminalExited",
                                        serde_json::Value::Null,
                                    ));
                                    break;
                                }
                                std::thread::sleep(Duration::from_millis(4));
                            }
                            Err(error) => {
                                let _ = events.send_ephemeral(WireDriverEvent::new(
                                    "terminalError",
                                    serde_json::Value::String(error.to_string()),
                                ));
                                break;
                            }
                        }
                    }
                })
                .context("start terminal output thread")?;

            Ok(Self {
                pty,
                stopped,
                reader: Some(reader),
                tail,
            })
        }

        pub fn write(&self, data: Vec<u8>) -> anyhow::Result<()> {
            if data.is_empty() {
                return Ok(());
            }
            let mut pty = self.pty.lock();
            pty.writer()
                .write_all(&data)
                .context("write terminal input")?;
            pty.writer().flush().context("flush terminal input")
        }

        pub fn resize(&self, cols: u16, rows: u16) {
            self.pty.lock().on_resize(window_size(cols, rows));
        }

        /// The PTY child's pid — the stats sampler attributes a process
        /// subtree to this terminal through it.
        pub fn child_pid(&self) -> u32 {
            self.pty.lock().child().id()
        }

        /// Drain the retained output tail — replayed to a client claiming
        /// the channel after a reconnect so its fresh emulator repaints.
        pub fn take_tail(&self) -> Vec<u8> {
            std::mem::take(&mut *self.tail.lock())
        }
    }

    impl Drop for DaemonTerminal {
        fn drop(&mut self) {
            self.stopped.store(true, Ordering::Release);
            // Alacritty's PTY destructor sends SIGHUP and then waits for the
            // child without a timeout. A shell can ignore or defer SIGHUP,
            // so bound its grace period before either join or PTY drop.
            // Hold the lock so the reader cannot reap the child between our
            // exit check and signal delivery.
            terminate_shell(&self.pty.lock());
            if let Some(reader) = self.reader.take() {
                let _ = reader.join();
            }
        }
    }

    fn terminate_shell(pty: &tty::Pty) {
        let child_pid = pty.child().id() as libc::pid_t;
        if child_has_exited(child_pid) {
            return;
        }
        unsafe {
            libc::kill(child_pid, libc::SIGHUP);
        }
        let deadline = Instant::now() + SHELL_SHUTDOWN_GRACE;
        while !child_has_exited(child_pid) {
            if Instant::now() >= deadline {
                unsafe {
                    libc::kill(child_pid, libc::SIGKILL);
                }
                break;
            }
            std::thread::sleep(Duration::from_millis(4));
        }
    }

    fn child_has_exited(child_pid: libc::pid_t) -> bool {
        // WNOWAIT leaves reaping to Alacritty's Child, keeping the PID owned
        // until its destructor has finished sending signals and waiting.
        let mut status: libc::siginfo_t = unsafe { std::mem::zeroed() };
        let result = unsafe {
            libc::waitid(
                libc::P_PID,
                child_pid as libc::id_t,
                &mut status,
                libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
            )
        };
        if result == 0 {
            unsafe { status.si_pid() != 0 }
        } else {
            // The output reader may already have reaped a naturally exited
            // shell via next_child_event before shutdown began.
            std::io::Error::last_os_error().raw_os_error() == Some(libc::ECHILD)
        }
    }

    fn window_size(cols: u16, rows: u16) -> WindowSize {
        WindowSize {
            num_lines: rows.max(MIN_ROWS),
            num_cols: cols.max(MIN_COLUMNS),
            cell_width: CELL_WIDTH,
            cell_height: CELL_HEIGHT,
        }
    }
}

#[cfg(unix)]
pub use platform::DaemonTerminal;

#[cfg(not(unix))]
pub struct DaemonTerminal;

#[cfg(not(unix))]
impl DaemonTerminal {
    pub fn open(
        _cwd: &Path,
        _cols: u16,
        _rows: u16,
        _events: impl TerminalEventSink,
    ) -> anyhow::Result<Self> {
        bail!("daemon terminals are not supported on this platform")
    }

    pub fn write(&self, _data: Vec<u8>) -> anyhow::Result<()> {
        bail!("daemon terminals are not supported on this platform")
    }

    pub fn open_command(
        _cwd: &Path,
        _cols: u16,
        _rows: u16,
        _events: impl TerminalEventSink,
        _command: &waku_protocol::custom_commands::CustomCommand,
        _scripts_directory: &Path,
    ) -> anyhow::Result<Self> {
        bail!("daemon terminals are not supported on this platform")
    }

    pub fn resize(&self, _cols: u16, _rows: u16) {}

    pub fn take_tail(&self) -> Vec<u8> {
        Vec::new()
    }

    pub fn child_pid(&self) -> u32 {
        0
    }
}

#[cfg(all(test, unix))]
mod writer_tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

    struct RetargetAlias {
        alias: PathBuf,
        replacement: PathBuf,
        observed: Arc<Mutex<Option<PathBuf>>>,
    }

    impl TerminalEventSink for RetargetAlias {
        fn before_spawn(&self, cwd: &std::path::Path) -> anyhow::Result<()> {
            *self.observed.lock().unwrap() = Some(cwd.to_owned());
            std::fs::remove_file(&self.alias)?;
            std::os::unix::fs::symlink(&self.replacement, &self.alias)?;
            Ok(())
        }
        fn send_ephemeral(&self, _: waku_protocol::WireDriverEvent) -> anyhow::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn terminal_spawn_uses_observed_directory_after_alias_retargeting() {
        let root =
            std::env::temp_dir().join(format!("terminal-observed-cwd-{}", uuid::Uuid::new_v4()));
        let original = root.join("original");
        let replacement = root.join("replacement");
        let alias = root.join("alias");
        let output = root.join("actual-cwd");
        std::fs::create_dir_all(&original).unwrap();
        std::fs::create_dir_all(&replacement).unwrap();
        std::os::unix::fs::symlink(&original, &alias).unwrap();
        let expected = original.canonicalize().unwrap();
        let observed = Arc::new(Mutex::new(None));
        let terminal = DaemonTerminal::open_with_shell(
            &alias,
            80,
            24,
            RetargetAlias {
                alias: alias.clone(),
                replacement: replacement.clone(),
                observed: observed.clone(),
            },
            alacritty_terminal::tty::Shell::new(
                "/bin/sh".into(),
                vec![
                    "-c".into(),
                    "pwd -P > \"$1\"".into(),
                    "terminal-cwd-fixture".into(),
                    output.to_string_lossy().into_owned(),
                ],
            ),
        )
        .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        let actual = loop {
            if let Ok(value) = std::fs::read_to_string(&output)
                && !value.trim().is_empty()
            {
                break value;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "terminal did not report its working directory"
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        };
        assert_eq!(
            alias.canonicalize().unwrap(),
            replacement.canonicalize().unwrap()
        );
        assert_eq!(observed.lock().unwrap().as_ref(), Some(&expected));
        assert_eq!(PathBuf::from(actual.trim()), expected);
        drop(terminal);
        std::fs::remove_dir_all(root).unwrap();
    }
}
