//! Cleanup for Windows process capture. Job membership survives the original
//! process exiting, and worker cancellation releases inherited pipe handles.

use std::ffi::c_void;
use std::io;
use std::os::windows::io::{AsHandle as _, AsRawHandle as _, FromRawHandle as _, OwnedHandle};
use std::process::{Child, ExitStatus};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use parking_lot::Mutex;

const THREAD_TERMINATE: u32 = 0x0001;
const WAIT_OBJECT_0: u32 = 0;
const WAIT_TIMEOUT: u32 = 258;

// These small, stable Kernel32 declarations keep capture independent of a new
// bindings dependency. All handles below are owned or borrowed Rust handles.
#[link(name = "kernel32")]
unsafe extern "system" {
    fn CreateJobObjectW(attributes: *const c_void, name: *const u16) -> *mut c_void;
    fn AssignProcessToJobObject(job: *mut c_void, process: *mut c_void) -> i32;
    fn TerminateJobObject(job: *mut c_void, exit_code: u32) -> i32;
    fn TerminateProcess(process: *mut c_void, exit_code: u32) -> i32;
    fn GetCurrentThreadId() -> u32;
    fn OpenThread(access: u32, inherit: i32, thread_id: u32) -> *mut c_void;
    fn CancelSynchronousIo(thread: *mut c_void) -> i32;
    fn WaitForSingleObject(handle: *mut c_void, milliseconds: u32) -> u32;
}

pub(super) struct ProcessTree {
    job: OwnedHandle,
    process: OwnedHandle,
}

impl ProcessTree {
    pub fn attach(child: &Child) -> io::Result<Self> {
        let process = child.as_handle().try_clone_to_owned()?;
        // SAFETY: null attributes request default security and a non-inherited
        // handle; the null name creates a fresh, unnamed job object.
        let job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if job.is_null() {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: CreateJobObjectW returned one valid handle owned by this call.
        let job = unsafe { OwnedHandle::from_raw_handle(job) };
        // Assign immediately after spawn. Stable std has no job-at-spawn API;
        // a descendant created before this call can escape the job, so pipe
        // cancellation below remains necessary even after job termination.
        // SAFETY: both borrowed handles remain alive throughout the call.
        if unsafe { AssignProcessToJobObject(job.as_raw_handle(), process.as_raw_handle()) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self { job, process })
    }

    pub fn terminate(&self) {
        // SAFETY: both handles remain owned here. TerminateJobObject reaches
        // descendants even when the root already exited; the direct process
        // call is a fallback. Already-exited processes require no action.
        unsafe {
            TerminateJobObject(self.job.as_raw_handle(), 1);
            TerminateProcess(self.process.as_raw_handle(), 1);
        }
    }
}

struct Worker {
    thread: OwnedHandle,
    finished: AtomicBool,
}

#[derive(Default)]
pub(super) struct Workers {
    workers: Mutex<Vec<Arc<Worker>>>,
}

pub(super) struct WorkerRegistration(Arc<Worker>);

impl Drop for WorkerRegistration {
    fn drop(&mut self) {
        self.0.finished.store(true, Ordering::Release);
    }
}

impl Workers {
    /// Register before checking cancellation or starting any pipe operation.
    pub fn register(&self) -> io::Result<WorkerRegistration> {
        // SAFETY: this thread is alive while its ID is opened. The returned
        // handle pins that particular thread, rather than a reusable thread ID.
        let thread = unsafe { OpenThread(THREAD_TERMINATE, 0, GetCurrentThreadId()) };
        if thread.is_null() {
            return Err(io::Error::last_os_error());
        }
        let worker = Arc::new(Worker {
            // SAFETY: OpenThread returned a valid handle with unique ownership.
            thread: unsafe { OwnedHandle::from_raw_handle(thread) },
            finished: AtomicBool::new(false),
        });
        self.workers.lock().push(Arc::clone(&worker));
        Ok(WorkerRegistration(worker))
    }

    pub fn cancel_and_wait(&self) {
        // The caller has already published cancellation. Later registrations
        // will observe it before attempting I/O; earlier registrations appear
        // in this snapshot. Handles remain alive until every worker is done.
        let workers = self.workers.lock().clone();
        loop {
            let mut pending = false;
            for worker in &workers {
                if worker.finished.load(Ordering::Acquire) {
                    continue;
                }
                pending = true;
                // SAFETY: the owned handle identifies only this capture's
                // dedicated pipe worker. It cannot cancel another task's I/O.
                // No pending I/O is harmless: retrying closes the race between
                // a worker's cancellation check and its blocking read/write.
                unsafe { CancelSynchronousIo(worker.thread.as_raw_handle()) };
            }
            if !pending {
                return;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}

pub(super) fn wait_for_child(
    child: &mut Child,
    deadline: Instant,
    cancelled: &AtomicBool,
) -> io::Result<ExitStatus> {
    loop {
        if cancelled.load(Ordering::Acquire) || Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "process wait deadline expired",
            ));
        }
        let milliseconds = deadline
            .saturating_duration_since(Instant::now())
            .as_millis()
            .clamp(1, 50) as u32;
        // SAFETY: the child owns this process handle throughout the wait.
        // Exit wakes immediately; the timeout only bounds cancellation latency.
        match unsafe { WaitForSingleObject(child.as_raw_handle(), milliseconds) } {
            WAIT_OBJECT_0 => return child.wait(),
            WAIT_TIMEOUT => {}
            _ => return Err(io::Error::last_os_error()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read as _, Write as _};
    use std::process::Stdio;

    use crate::capture::{CaptureError, CaptureOptions, OutputLimit, capture};

    const CHILD_MODE: &str = "GODDARD_WINDOWS_CAPTURE_CHILD_MODE";
    const CHILD_PID_FILE: &str = "GODDARD_WINDOWS_CAPTURE_CHILD_PID_FILE";
    const CHILD_TEST: &str = "capture::windows::tests::child_fixture";

    #[test]
    fn child_fixture() {
        let Ok(mode) = std::env::var(CHILD_MODE) else {
            return;
        };
        if mode == "orphan" {
            // Capture writes stdin only after job assignment. This handshake
            // tests cleanup of a job-owned descendant after its root exits.
            let mut ready = [0];
            std::io::stdin().read_exact(&mut ready).unwrap();
            let child = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", CHILD_TEST, "--nocapture"])
                .env(CHILD_MODE, "sleep")
                .stdin(Stdio::null())
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit())
                .spawn()
                .unwrap();
            std::fs::write(
                std::env::var_os(CHILD_PID_FILE).unwrap(),
                child.id().to_string(),
            )
            .unwrap();
            return;
        }
        if mode == "overflow" {
            std::io::stdout().write_all(&[b'x'; 2048]).unwrap();
            std::io::stdout().flush().unwrap();
        }
        std::thread::sleep(Duration::from_secs(30));
    }

    fn command(mode: &str) -> crate::command_env::Proc {
        let mut command = crate::command_env::plain_command(std::env::current_exe().unwrap());
        command
            .args(["--exact", CHILD_TEST, "--nocapture"])
            .env(CHILD_MODE, mode);
        command
    }

    fn options() -> CaptureOptions {
        CaptureOptions {
            deadline: Instant::now() + Duration::from_secs(1),
            stdout: OutputLimit::Exact(1024),
            stderr: OutputLimit::Prefix(1024),
        }
    }

    #[test]
    fn deadline_cancels_a_blocked_stdin_writer() {
        let started = Instant::now();
        let result = capture(
            &mut command("sleep"),
            options(),
            Some(&vec![b'x'; 1024 * 1024]),
        );
        assert!(matches!(result, Err(CaptureError::Deadline)));
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn output_overflow_cancels_other_pipe_workers() {
        let started = Instant::now();
        let mut limits = options();
        limits.deadline = Instant::now() + Duration::from_secs(10);
        let result = capture(&mut command("overflow"), limits, None);
        assert!(matches!(
            result,
            Err(CaptureError::OutputLimit {
                stream: "stdout",
                ..
            })
        ));
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn deadline_stops_a_descendant_holding_stdout_after_parent_exit() {
        let pid_file = std::env::temp_dir().join(format!(
            "goddard-windows-capture-{}.pid",
            uuid::Uuid::new_v4()
        ));
        let mut command = command("orphan");
        command.env(CHILD_PID_FILE, &pid_file);
        let started = Instant::now();
        let result = capture(&mut command, options(), Some(b"!"));
        assert!(matches!(result, Err(CaptureError::Deadline)));
        assert!(started.elapsed() < Duration::from_secs(5));
        let pid: u32 = std::fs::read_to_string(&pid_file).unwrap().parse().unwrap();
        std::fs::remove_file(pid_file).unwrap();

        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn OpenProcess(access: u32, inherit: i32, process_id: u32) -> *mut c_void;
        }
        const SYNCHRONIZE: u32 = 0x0010_0000;
        // SAFETY: this opens only the test fixture's process for a wait probe.
        let process = unsafe { OpenProcess(SYNCHRONIZE, 0, pid) };
        if process.is_null() {
            // ERROR_INVALID_PARAMETER means the process no longer exists.
            assert_eq!(io::Error::last_os_error().raw_os_error(), Some(87));
        } else {
            // SAFETY: OpenProcess returned a valid, uniquely owned handle.
            let process = unsafe { OwnedHandle::from_raw_handle(process) };
            // SAFETY: the handle is live and the finite wait only observes exit.
            assert_eq!(
                unsafe { WaitForSingleObject(process.as_raw_handle(), 1000) },
                WAIT_OBJECT_0
            );
        }
    }
}
