//! Explicit capture policies for finite Git reads and mutations.

use std::cell::Cell;
use std::process::Output;
use std::time::{Duration, Instant};

pub use waku_exec::capture::{CaptureError, CaptureOptions, OutputLimit};
use waku_exec::command_env::Proc;

pub const READ_TIMEOUT: Duration = Duration::from_secs(30);
pub const MUTATION_TIMEOUT: Duration = Duration::from_secs(120);
pub const MAX_METADATA_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_DIAGNOSTIC_BYTES: usize = 256 * 1024;

thread_local! {
    static REQUEST_DEADLINE: Cell<Option<Instant>> = const { Cell::new(None) };
}

struct RestoreDeadline(Option<Instant>);

impl Drop for RestoreDeadline {
    fn drop(&mut self) {
        REQUEST_DEADLINE.with(|deadline| deadline.set(self.0));
    }
}

/// Give all commands in a synchronous Git operation the same remaining
/// budget, including nested reads, snapshot capture, and diff fallback.
pub fn with_deadline<T>(deadline: Instant, operation: impl FnOnce() -> T) -> T {
    let previous = REQUEST_DEADLINE.with(|current| {
        current.replace(Some(
            current
                .get()
                .map_or(deadline, |parent| parent.min(deadline)),
        ))
    });
    let _restore = RestoreDeadline(previous);
    operation()
}

pub trait GitCommand {
    fn git_read_output(&mut self) -> Result<Output, CaptureError>;
    fn git_mutation_output(&mut self) -> Result<Output, CaptureError>;
}

impl GitCommand for Proc {
    fn git_read_output(&mut self) -> Result<Output, CaptureError> {
        read(self)
    }

    fn git_mutation_output(&mut self) -> Result<Output, CaptureError> {
        mutation(self)
    }
}

/// Complete machine-readable output or an error; never a successful prefix.
pub fn read(command: &mut Proc) -> Result<Output, CaptureError> {
    read_until(command, Instant::now() + READ_TIMEOUT, MAX_METADATA_BYTES)
}

pub fn read_until(
    command: &mut Proc,
    deadline: Instant,
    maximum_bytes: usize,
) -> Result<Output, CaptureError> {
    command.env("GIT_OPTIONAL_LOCKS", "0");
    output(command, deadline, OutputLimit::Exact(maximum_bytes), None)
}

/// Mutations drain bounded diagnostic prefixes without stopping early when
/// output is verbose. A deadline is a failure, never an instruction to retry.
pub fn mutation(command: &mut Proc) -> Result<Output, CaptureError> {
    output(
        command,
        Instant::now() + MUTATION_TIMEOUT,
        OutputLimit::Prefix(MAX_DIAGNOSTIC_BYTES),
        None,
    )
}

/// Plumbing that writes a private index can request exact output without
/// disabling Git's optional index refresh through the read profile.
pub fn output(
    command: &mut Proc,
    deadline: Instant,
    stdout: OutputLimit,
    input: Option<&[u8]>,
) -> Result<Output, CaptureError> {
    let deadline = REQUEST_DEADLINE.with(|current| {
        current
            .get()
            .map_or(deadline, |parent| parent.min(deadline))
    });
    command
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_EDITOR", "true");
    let mut captured = waku_exec::capture::capture(
        command,
        CaptureOptions {
            deadline,
            stdout,
            stderr: OutputLimit::Prefix(MAX_DIAGNOSTIC_BYTES),
        },
        input,
    )?;
    if captured.stdout_truncated {
        captured
            .stdout
            .extend_from_slice(b"\n[diagnostic output truncated]\n");
    }
    if captured.stderr_truncated {
        captured
            .stderr
            .extend_from_slice(b"\n[diagnostic output truncated]\n");
    }
    Ok(Output {
        status: captured.status,
        stdout: captured.stdout,
        stderr: captured.stderr,
    })
}
