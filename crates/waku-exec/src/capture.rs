//! Deadline-bound subprocess capture. Exact output either fits or fails;
//! diagnostic prefixes keep draining so a noisy mutation can finish normally.

use std::fmt;
use std::io::{self, Read, Write};
use std::process::{ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::Instant;

use crate::command_env::{BoundedChild, Proc};

#[cfg(windows)]
mod windows;

#[derive(Clone, Copy, Debug)]
pub enum OutputLimit {
    Exact(usize),
    Prefix(usize),
}

#[derive(Clone, Copy, Debug)]
pub struct CaptureOptions {
    /// Includes queueing, execution, and all pipe I/O.
    pub deadline: Instant,
    pub stdout: OutputLimit,
    pub stderr: OutputLimit,
}

#[derive(Debug)]
pub struct CapturedOutput {
    pub status: ExitStatus,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
}

#[derive(Debug)]
pub enum CaptureError {
    Deadline,
    OutputLimit { stream: &'static str, limit: usize },
    Io(io::Error),
}

impl fmt::Display for CaptureError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Deadline => formatter.write_str("process deadline expired"),
            Self::OutputLimit { stream, limit } => {
                write!(
                    formatter,
                    "process {stream} exceeded its {limit}-byte limit"
                )
            }
            Self::Io(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for CaptureError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

impl From<io::Error> for CaptureError {
    fn from(error: io::Error) -> Self {
        if error.kind() == io::ErrorKind::TimedOut {
            Self::Deadline
        } else {
            Self::Io(error)
        }
    }
}

struct StreamOutput {
    bytes: Vec<u8>,
    truncated: bool,
}

enum Event {
    Exited(io::Result<()>),
    Stdout(Result<StreamOutput, CaptureError>),
    Stderr(Result<StreamOutput, CaptureError>),
    Input(io::Result<()>),
}

// Keep the spawn permit until the child is reaped, including worker-start
// errors and unwinding. The process-group guard below stops descendants.
struct ReapChild(BoundedChild);

impl Drop for ReapChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Capture a finite command under one absolute deadline. On failure the
/// process group is stopped and the child reaped before its permit is released.
pub fn capture(
    command: &mut Proc,
    options: CaptureOptions,
    input: Option<&[u8]>,
) -> Result<CapturedOutput, CaptureError> {
    #[cfg(unix)]
    command.process_group(0);
    command
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut owned_child = ReapChild(command.spawn_bounded_until(options.deadline)?);
    let child = &mut owned_child.0;
    #[cfg(windows)]
    let process_tree = match windows::ProcessTree::attach(child.child_mut()) {
        Ok(tree) => tree,
        Err(error) => {
            // A capture without a cleanup handle cannot safely start its
            // workers. Windows has no zombie process to reap after close.
            let _ = child.kill();
            let _ = child.try_wait();
            return Err(error.into());
        }
    };
    #[cfg(unix)]
    let pid = child.id();
    let stdout = child.stdout().expect("piped stdout requested");
    let stderr = child.stderr().expect("piped stderr requested");
    let stdin = child.stdin();
    let cancelled = AtomicBool::new(false);
    #[cfg(windows)]
    let workers = windows::Workers::default();
    let (sender, receiver) = mpsc::channel();

    // A blocking waiter signals completion directly: short Git commands do
    // not incur a fixed polling interval. The scoped threads finish before
    // `child` drops, keeping its spawn permit through cleanup and pipe reads.
    let result = std::thread::scope(|scope| {
        let cancellation = &cancelled;
        #[cfg(windows)]
        let workers = &workers;
        let mut cleanup = ProcessCleanup {
            #[cfg(unix)]
            pid,
            cancelled: &cancelled,
            #[cfg(windows)]
            process_tree: &process_tree,
            #[cfg(windows)]
            workers,
            armed: true,
        };
        let exited = sender.clone();
        #[cfg(not(unix))]
        let waiting_child = &mut *child;
        std::thread::Builder::new()
            .name("process-wait".into())
            .spawn_scoped(scope, move || {
                #[cfg(windows)]
                let result = windows::wait_for_child(
                    waiting_child.child_mut(),
                    options.deadline,
                    cancellation,
                )
                .map(|_| ());
                #[cfg(unix)]
                let result = observe_child_exit(pid);
                #[cfg(not(any(unix, windows)))]
                let result = waiting_child.wait().map(|_| ());
                let _ = exited.send(Event::Exited(result));
            })?;
        let output = sender.clone();
        std::thread::Builder::new()
            .name("process-stdout".into())
            .spawn_scoped(scope, move || {
                let result = (|| {
                    #[cfg(windows)]
                    let _registration = workers.register()?;
                    read_stream(
                        stdout,
                        options.stdout,
                        "stdout",
                        options.deadline,
                        cancellation,
                    )
                })();
                let _ = output.send(Event::Stdout(result));
            })?;
        let errors = sender.clone();
        std::thread::Builder::new()
            .name("process-stderr".into())
            .spawn_scoped(scope, move || {
                let result = (|| {
                    #[cfg(windows)]
                    let _registration = workers.register()?;
                    read_stream(
                        stderr,
                        options.stderr,
                        "stderr",
                        options.deadline,
                        cancellation,
                    )
                })();
                let _ = errors.send(Event::Stderr(result));
            })?;
        if let Some(input) = input {
            let mut stdin = stdin.expect("piped stdin requested");
            let written = sender.clone();
            std::thread::Builder::new()
                .name("process-stdin".into())
                .spawn_scoped(scope, move || {
                    let result = (|| {
                        #[cfg(windows)]
                        let _registration = workers.register()?;
                        write_input(&mut stdin, input, options.deadline, cancellation)
                    })();
                    let _ = written.send(Event::Input(result));
                })?;
        }
        drop(sender);
        let mut exited = false;
        let mut stdout = None;
        let mut stderr = None;
        let mut input_done = input.is_none();
        while !exited || stdout.is_none() || stderr.is_none() || !input_done {
            let remaining = options.deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(CaptureError::Deadline);
            }
            let event = receiver
                .recv_timeout(remaining)
                .map_err(|error| match error {
                    mpsc::RecvTimeoutError::Timeout => CaptureError::Deadline,
                    mpsc::RecvTimeoutError::Disconnected => {
                        CaptureError::Io(io::Error::other("process capture worker disconnected"))
                    }
                })?;
            match event {
                Event::Exited(result) => {
                    result?;
                    exited = true;
                }
                Event::Stdout(result) => stdout = Some(result?),
                Event::Stderr(result) => stderr = Some(result?),
                Event::Input(result) => {
                    result?;
                    input_done = true;
                }
            }
        }
        cleanup.armed = false;
        let stdout = stdout.expect("stdout completed");
        let stderr = stderr.expect("stderr completed");
        Ok((stdout, stderr))
    });
    // Unix observes exit without reaping, pinning the PID while pipe cleanup
    // may still need to signal its group. Reap only after every worker joins.
    let status = child.wait();
    match result {
        Ok((stdout, stderr)) => Ok(CapturedOutput {
            status: status?,
            stdout: stdout.bytes,
            stderr: stderr.bytes,
            stdout_truncated: stdout.truncated,
            stderr_truncated: stderr.truncated,
        }),
        Err(error) => Err(error),
    }
}

#[cfg(unix)]
fn observe_child_exit(pid: u32) -> io::Result<()> {
    loop {
        // SAFETY: waitid initializes siginfo for our live, unreaped child.
        // WNOWAIT preserves its identity until the owning thread calls wait.
        let mut info = unsafe { std::mem::zeroed::<libc::siginfo_t>() };
        let result = unsafe {
            libc::waitid(
                libc::P_PID,
                pid as libc::id_t,
                &mut info,
                libc::WEXITED | libc::WNOWAIT,
            )
        };
        if result == 0 {
            return Ok(());
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(error);
        }
    }
}

struct ProcessCleanup<'a> {
    #[cfg(unix)]
    pid: u32,
    cancelled: &'a AtomicBool,
    #[cfg(windows)]
    process_tree: &'a windows::ProcessTree,
    #[cfg(windows)]
    workers: &'a windows::Workers,
    armed: bool,
}

impl Drop for ProcessCleanup<'_> {
    fn drop(&mut self) {
        if self.armed {
            self.cancelled.store(true, Ordering::Release);
            #[cfg(unix)]
            terminate_process_group(self.pid);
            #[cfg(windows)]
            {
                self.process_tree.terminate();
                self.workers.cancel_and_wait();
            }
        }
    }
}

#[cfg(unix)]
fn terminate_process_group(pid: u32) {
    // SAFETY: these are signal-only calls targeting the child/group created
    // above. No Rust memory is accessed by libc; ESRCH means it already exited.
    unsafe {
        libc::kill(-(pid as i32), libc::SIGKILL);
        libc::kill(pid as i32, libc::SIGKILL);
    }
}

#[cfg(unix)]
trait Pipe: std::os::fd::AsRawFd {}
#[cfg(unix)]
impl<T: std::os::fd::AsRawFd> Pipe for T {}
#[cfg(not(unix))]
trait Pipe {}
#[cfg(not(unix))]
impl<T> Pipe for T {}

fn configure_pipe(pipe: &impl Pipe) -> io::Result<()> {
    #[cfg(unix)]
    {
        // SAFETY: the pipe owns a live descriptor throughout this call. Only
        // its nonblocking flag changes; ownership stays with the Rust handle.
        let flags = unsafe { libc::fcntl(pipe.as_raw_fd(), libc::F_GETFL) };
        if flags < 0
            || unsafe { libc::fcntl(pipe.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0
        {
            return Err(io::Error::last_os_error());
        }
    }
    #[cfg(not(unix))]
    let _ = pipe;
    Ok(())
}

fn wait_ready(
    pipe: &impl Pipe,
    writing: bool,
    deadline: Instant,
    cancelled: &AtomicBool,
) -> io::Result<()> {
    loop {
        if cancelled.load(Ordering::Acquire) || Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "process pipe deadline expired",
            ));
        }
        #[cfg(unix)]
        {
            let mut descriptor = libc::pollfd {
                fd: pipe.as_raw_fd(),
                events: if writing { libc::POLLOUT } else { libc::POLLIN },
                revents: 0,
            };
            // Cancellation checks have a bounded cadence; normal output and
            // EOF wake poll immediately, without adding latency to fast calls.
            let millis = deadline
                .saturating_duration_since(Instant::now())
                .as_millis()
                .clamp(1, 50) as i32;
            // SAFETY: descriptor points to one initialized pollfd for a live
            // pipe; poll only updates its revents field during this call.
            let ready = unsafe { libc::poll(&mut descriptor, 1, millis) };
            if ready > 0 {
                return Ok(());
            }
            if ready < 0 {
                let error = io::Error::last_os_error();
                if error.kind() != io::ErrorKind::Interrupted {
                    return Err(error);
                }
            }
        }
        #[cfg(not(unix))]
        {
            let _ = (pipe, writing);
            return Ok(());
        }
    }
}

fn read_stream(
    mut pipe: impl Read + Pipe,
    policy: OutputLimit,
    stream: &'static str,
    deadline: Instant,
    cancelled: &AtomicBool,
) -> Result<StreamOutput, CaptureError> {
    configure_pipe(&pipe)?;
    let limit = match policy {
        OutputLimit::Exact(limit) | OutputLimit::Prefix(limit) => limit,
    };
    let mut output = StreamOutput {
        bytes: Vec::new(),
        truncated: false,
    };
    let mut chunk = [0; 8192];
    loop {
        wait_ready(&pipe, false, deadline, cancelled)?;
        let count = match pipe.read(&mut chunk) {
            Ok(0) => return Ok(output),
            Ok(count) => count,
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::Interrupted | io::ErrorKind::WouldBlock
                ) =>
            {
                continue;
            }
            Err(error) => return Err(error.into()),
        };
        let keep = count.min(limit.saturating_sub(output.bytes.len()));
        output.bytes.extend_from_slice(&chunk[..keep]);
        if keep < count {
            if matches!(policy, OutputLimit::Exact(_)) {
                return Err(CaptureError::OutputLimit { stream, limit });
            }
            output.truncated = true;
        }
    }
}

fn write_input(
    pipe: &mut (impl Write + Pipe),
    mut input: &[u8],
    deadline: Instant,
    cancelled: &AtomicBool,
) -> io::Result<()> {
    configure_pipe(pipe)?;
    while !input.is_empty() {
        wait_ready(pipe, true, deadline, cancelled)?;
        match pipe.write(&input[..input.len().min(8192)]) {
            Ok(0) => {
                return Err(io::Error::new(
                    io::ErrorKind::WriteZero,
                    "process stdin closed",
                ));
            }
            Ok(written) => input = &input[written..],
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::Interrupted | io::ErrorKind::WouldBlock
                ) =>
            {
                ()
            }
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::time::Duration;

    fn options(stdout: OutputLimit) -> CaptureOptions {
        CaptureOptions {
            deadline: Instant::now() + Duration::from_secs(5),
            stdout,
            stderr: OutputLimit::Prefix(1024),
        }
    }

    #[test]
    fn exact_capture_preserves_both_streams_and_input() {
        let mut command = crate::command_env::plain_command("/bin/sh");
        command.args(["-c", "cat; printf diagnostic >&2; exit 7"]);
        let output = capture(
            &mut command,
            options(OutputLimit::Exact(1024)),
            Some(b"input\0bytes\n"),
        )
        .unwrap();
        assert_eq!(output.status.code(), Some(7));
        assert_eq!(output.stdout, b"input\0bytes\n");
        assert_eq!(output.stderr, b"diagnostic");
        assert!(!output.stdout_truncated);
    }

    #[test]
    fn exact_output_overflow_fails_instead_of_returning_a_prefix() {
        let mut command = crate::command_env::plain_command("/bin/sh");
        command.args(["-c", "printf 12345; sleep 30"]);
        let started = Instant::now();
        let error = capture(&mut command, options(OutputLimit::Exact(4)), None).unwrap_err();
        assert!(matches!(
            error,
            CaptureError::OutputLimit {
                stream: "stdout",
                limit: 4
            }
        ));
        assert!(started.elapsed() < Duration::from_secs(3));
    }

    #[test]
    fn diagnostic_prefix_keeps_draining_until_the_command_finishes() {
        let mut command = crate::command_env::plain_command("/bin/sh");
        command.args(["-c", "head -c 65536 /dev/zero; printf finished >&2"]);
        let output = capture(&mut command, options(OutputLimit::Prefix(4)), None).unwrap();
        assert!(output.status.success());
        assert_eq!(output.stdout, [0; 4]);
        assert!(output.stdout_truncated);
        assert_eq!(output.stderr, b"finished");
    }

    #[test]
    fn deadline_covers_a_blocked_input_writer() {
        let mut command = crate::command_env::plain_command("/bin/sh");
        command.args(["-c", "sleep 30"]);
        let mut limits = options(OutputLimit::Exact(1024));
        limits.deadline = Instant::now() + Duration::from_millis(100);
        let started = Instant::now();
        let error = capture(&mut command, limits, Some(&vec![b'x'; 1024 * 1024])).unwrap_err();
        assert!(matches!(error, CaptureError::Deadline));
        assert!(started.elapsed() < Duration::from_secs(3));
    }

    #[test]
    fn deadline_stops_descendants_holding_pipes_after_the_parent_exits() {
        let pid_file =
            std::env::temp_dir().join(format!("goddard-capture-{}.pid", uuid::Uuid::new_v4()));
        let mut command = crate::command_env::plain_command("/bin/sh");
        command.args([
            "-c",
            "sleep 30 & printf '%s' \"$!\" > \"$1\"",
            "capture-test",
        ]);
        command.arg(&pid_file);
        let mut limits = options(OutputLimit::Exact(1024));
        limits.deadline = Instant::now() + Duration::from_millis(200);
        let started = Instant::now();
        let error = capture(&mut command, limits, None).unwrap_err();
        assert!(matches!(error, CaptureError::Deadline));
        assert!(started.elapsed() < Duration::from_secs(3));
        let pid = std::fs::read_to_string(&pid_file).unwrap();
        std::fs::remove_file(pid_file).unwrap();
        let status = std::process::Command::new("/bin/ps")
            .args(["-o", "stat=", "-p", &pid])
            .output()
            .unwrap();
        let status = String::from_utf8_lossy(&status.stdout);
        assert!(
            status.trim().is_empty() || status.trim_start().starts_with('Z'),
            "descendant survived: {status}"
        );
    }
}
