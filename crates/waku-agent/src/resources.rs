//! Shell runner: acquire once, wait without model retries, register a gated process
//! group before exec, and keep supervising until it exits or ownership is revoked.
use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};
use uuid::Uuid;
use waku_protocol::{Command, ResponsePayload, resources::*};

static INTERRUPTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
#[cfg(unix)]
extern "C" fn interrupt(_: libc::c_int) {
    INTERRUPTED.store(true, std::sync::atomic::Ordering::Relaxed);
}
fn interrupted() -> bool {
    INTERRUPTED.load(std::sync::atomic::Ordering::Relaxed)
}

const PARENT_ENV: &str = waku_protocol::AGENT_RESOURCE_RESERVATION_ENV;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Acquire {
    resources: ResourceSet,
    purpose: String,
    #[serde(default = "default_wait")]
    wait_seconds: u32,
    #[serde(default)]
    parent: Option<Uuid>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Id {
    id: Uuid,
}

fn request(operation: ResourceOperation) -> Result<ResourceStatus> {
    match super::connect()?.request(
        super::request_session_id(),
        Uuid::nil(),
        Command::AgentResources { operation },
    )? {
        ResponsePayload::AgentResources { status } => Ok(status),
        other => bail!("unexpected resource response: {other:?}"),
    }
}
fn reservation(status: &ResourceStatus, id: Uuid) -> Result<&Reservation> {
    status
        .reservations
        .iter()
        .find(|r| r.id == id)
        .context("reservation ended or wait timed out")
}
fn waiting(r: &Reservation, status: &ResourceStatus) -> String {
    let label = if r.resources.resident_devices > 0 {
        r.resources.exclusive.join(", ")
    } else if r.resources.native_builds > 0 {
        "native build".into()
    } else {
        "desktop input".into()
    };
    let owner = status
        .reservations
        .iter()
        .find(|h| h.id != r.id && h.granted_at.is_some());
    match owner {
        Some(h) => format!(
            "Waiting for {label}—held by task {} ({})",
            h.task, h.purpose
        ),
        None => format!("Waiting for {label}—queued for host capacity or a user-owned device"),
    }
}
fn acquire(mut payload: Acquire, holder_pid: u32) -> Result<(Uuid, bool)> {
    if payload.parent.is_none() {
        payload.parent = std::env::var(PARENT_ENV)
            .ok()
            .map(|value| value.parse())
            .transpose()
            .context("invalid inherited resource reservation")?;
    }
    let mut status = request(ResourceOperation::Acquire {
        resources: payload.resources,
        purpose: payload.purpose,
        holder_pid,
        wait_seconds: payload.wait_seconds,
        parent: payload.parent,
    })?;
    let id = status.request_id.context("broker omitted reservation id")?;
    let borrowed = status.borrowed;
    let deadline = Instant::now() + Duration::from_secs(u64::from(payload.wait_seconds));
    let mut last_title = String::new();
    loop {
        if interrupted() {
            let _ = request(ResourceOperation::Cancel { id });
            bail!("resource wait interrupted");
        }
        let r = reservation(&status, id)?;
        if r.cancelled || r.released {
            bail!("reservation cancelled");
        }
        if r.granted_at.is_some() {
            return Ok((id, borrowed));
        }
        let title = waiting(r, &status);
        if title != last_title {
            eprintln!("{title} [request {id}]");
            last_title = title;
        }
        if Instant::now() >= deadline {
            let _ = request(ResourceOperation::Cancel { id });
            bail!("resource wait timed out [request {id}]");
        }
        std::thread::sleep(Duration::from_millis(500));
        status = request(ResourceOperation::Status { id: Some(id) })?;
    }
}

pub fn command(mut arguments: impl Iterator<Item = String>) -> Result<()> {
    #[cfg(unix)]
    unsafe {
        let handler = interrupt as *const () as libc::sighandler_t;
        libc::signal(libc::SIGINT, handler);
        libc::signal(libc::SIGTERM, handler);
    }
    let action = arguments
        .next()
        .context("resource requires acquire, run, release, cancel, or status")?;
    #[cfg(not(unix))]
    bail!("resource supervision currently requires a Unix host");
    match action.as_str() {
        "status" => {
            if arguments.next().is_some() {
                bail!("resource status takes no arguments");
            }
            println!(
                "{}",
                serde_json::to_string_pretty(&request(ResourceOperation::Status { id: None })?)?
            );
        }
        "release" | "cancel" => {
            let payload: Id =
                serde_json::from_str(&arguments.next().context("expected JSON with id")?)?;
            if arguments.next().is_some() {
                bail!("expected exactly one JSON argument");
            }
            // Nested tooling must not release the outer command's ownership.
            if std::env::var(PARENT_ENV).ok().as_deref() == Some(&payload.id.to_string()) {
                bail!("nested tooling cannot release or cancel its inherited reservation");
            }
            let operation = if action == "release" {
                ResourceOperation::Release { id: payload.id }
            } else {
                ResourceOperation::Cancel { id: payload.id }
            };
            println!("{}", serde_json::to_string_pretty(&request(operation)?)?);
        }
        "acquire" | "run" => {
            let payload: Acquire =
                serde_json::from_str(&arguments.next().context("expected acquisition JSON")?)?;
            if action == "acquire" {
                if arguments.next().is_some() {
                    bail!("resource acquire takes one JSON argument");
                }
                #[cfg(unix)]
                let holder = unsafe { libc::getppid() as u32 };
                #[cfg(not(unix))]
                let holder = std::process::id();
                let (id, borrowed) = acquire(payload, holder)?;
                println!("{}", serde_json::json!({"id": id, "borrowed": borrowed}));
            } else {
                if arguments.next().as_deref() != Some("--") {
                    bail!("resource run requires -- followed by an executable and arguments");
                }
                let argv: Vec<_> = arguments.collect();
                if argv.is_empty() {
                    bail!("resource run requires a command");
                }
                let (id, borrowed) = acquire(payload, std::process::id())?;
                #[cfg(unix)]
                run(id, borrowed, argv)?;
            }
        }
        _ => bail!("unknown resource action {action}"),
    }
    Ok(())
}

#[cfg(unix)]
fn run(id: Uuid, borrowed: bool, argv: Vec<String>) -> Result<()> {
    use std::{
        os::unix::{fs::DirBuilderExt, process::CommandExt},
        process::Command,
    };
    if borrowed {
        // The outer process group and supervisor already cover the nested command.
        let status = Command::new(&argv[0]).args(&argv[1..]).status()?;
        std::process::exit(status.code().unwrap_or(1));
    }
    let gate = std::env::temp_dir().join(format!("goddard-resource-{}", Uuid::new_v4()));
    std::fs::DirBuilder::new().mode(0o700).create(&gate)?;
    let result = (|| -> Result<i32> {
        let mut child = Command::new(std::env::current_exe()?)
            .arg("__resource_exec")
            .arg(&gate)
            .arg(serde_json::to_string(&argv)?)
            .env(PARENT_ENV, id.to_string())
            .process_group(0)
            .spawn()?;
        let pid = child.id();
        // Child cannot start user tooling until its process group is durably recorded.
        if let Err(error) = request(ResourceOperation::Attach {
            id,
            workload_pid: pid,
        }) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
        std::fs::write(gate.join("go"), b"")?;
        loop {
            if let Some(status) = child.try_wait()? {
                return Ok(status.code().unwrap_or(1));
            }
            match request(ResourceOperation::Status { id: Some(id) })
                .and_then(|s| Ok(interrupted() || reservation(&s, id)?.cancelled))
            {
                Ok(false) => {}
                // Task credentials die with the provider; loss of authentication cancels owned work.
                Ok(true) | Err(_) => {
                    unsafe {
                        libc::kill(-(pid as i32), libc::SIGTERM);
                    }
                    let deadline = Instant::now() + Duration::from_secs(2);
                    let mut reaped = false;
                    while Instant::now() < deadline {
                        if child.try_wait()?.is_some() {
                            reaped = true;
                            break;
                        }
                        std::thread::sleep(Duration::from_millis(50));
                    }
                    // Keep the leader unreaped until escalation: its PID cannot
                    // be reused by an unrelated process group in this interval.
                    if !reaped {
                        unsafe {
                            libc::kill(-(pid as i32), libc::SIGKILL);
                        }
                        let _ = child.wait();
                    }
                    bail!("resource workload cancelled or task ownership lost");
                }
            }
            std::thread::sleep(Duration::from_millis(500));
        }
    })();
    let _ = std::fs::remove_dir_all(gate);
    let _ = request(ResourceOperation::Release { id });
    match result {
        Ok(code) => std::process::exit(code),
        Err(error) => Err(error),
    }
}

/// Private exec gate. Uses inherited standard streams so shell pipelines stay usable.
#[cfg(unix)]
pub fn exec_child(mut arguments: impl Iterator<Item = String>) -> Result<()> {
    use std::{os::unix::process::CommandExt, process::Command};
    let gate = PathBuf::from(arguments.next().context("missing gate")?);
    let argv: Vec<String> = serde_json::from_str(&arguments.next().context("missing argv")?)?;
    let parent = unsafe { libc::getppid() };
    let deadline = Instant::now() + Duration::from_secs(30);
    while !gate.join("go").exists() {
        if Instant::now() >= deadline || unsafe { libc::getppid() } != parent {
            bail!("workload registration abandoned");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let program = argv.first().context("empty argv")?;
    Err(Command::new(program).args(&argv[1..]).exec().into())
}
