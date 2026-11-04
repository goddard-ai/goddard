use std::io::Write as _;
use std::net::{SocketAddr, TcpListener};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anyhow::{Context as _, anyhow, bail};
use waku_protocol::{
    AGENT_TASK_ENV, AGENT_TOKEN_ENV, DAEMON_ADDRESS_ENV, DAEMON_TOKEN_ENV, DaemonReady,
    PROTOCOL_VERSION,
};

fn main() -> anyhow::Result<()> {
    // Register the embedded locale catalog with waku-protocol before any wire
    // type renders text in this process.
    waku_daemon::i18n::install();
    waku_daemon::command_env::raise_open_file_limit();
    let mut migration = waku_protocol::migration::migrate_home_directory();
    let task_path = waku_daemon::persistence::StateStore::default_path();
    // One daemon owns a data directory: a second host opening the same
    // app.db advances the Boss revision and wedges the live host's saves.
    // Take the lock before data-directory migrations touch the dir.
    let _data_dir_lock = lock_data_directory(task_path.parent().unwrap_or(&task_path))?;
    migration.extend(waku_daemon::migration::migrate_data_directory());
    for failure in &migration.failures {
        eprintln!(
            "Goddard: could not migrate {} to {} ({:#}); starting with fresh state — restart to retry",
            failure.legacy.display(),
            failure.destination.display(),
            failure.error
        );
    }
    for skipped in &migration.skipped {
        eprintln!("Goddard: left legacy item {} unmigrated", skipped.display());
    }
    let arguments = Arguments::parse(std::env::args().skip(1))?;
    let token = std::env::var(DAEMON_TOKEN_ENV)
        .context("Goddard daemon authentication token is missing")?;
    // The bearer capability belongs only to this server process. Remove it
    // before any provider or workspace subprocess can inherit the daemon's
    // environment — and drop any agent credential this process itself
    // inherited, since scoped tokens are minted per provider session and
    // never pass through unchanged.
    unsafe {
        std::env::remove_var(DAEMON_TOKEN_ENV);
        std::env::remove_var(AGENT_TOKEN_ENV);
        std::env::remove_var(AGENT_TASK_ENV);
        std::env::remove_var(DAEMON_ADDRESS_ENV);
    }
    let listener = TcpListener::bind(&arguments.bind)
        .with_context(|| format!("could not bind Goddard daemon to {}", arguments.bind))?;
    let address = listener.local_addr()?;
    ensure_bind_allowed(address, arguments.allow_non_loopback)?;
    let shutdown = Arc::new(AtomicBool::new(false));
    if let Some(parent_pid) = arguments.parent_pid {
        let monitor_shutdown = shutdown.clone();
        std::thread::Builder::new()
            .name("goddard-daemon-parent".into())
            .spawn(move || {
                while !monitor_shutdown.load(Ordering::Acquire) {
                    if !waku_protocol::pid::is_alive(parent_pid) {
                        monitor_shutdown.store(true, Ordering::Release);
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(500));
                }
            })?;
    }

    // Request-thread panics unwind without killing the daemon — the log is
    // the only trace a wedged handler leaves.
    waku_daemon::stats::install_panic_log(task_path.parent().unwrap_or(&task_path));
    let settings = waku_daemon::DaemonSettingsStore::open_with_legacy(
        waku_daemon::DaemonSettings::default_path(),
        [task_path.with_file_name("settings.json")],
    )
    .context("could not load daemon settings")?;
    let task_store = waku_daemon::persistence::StateStore::daemon(task_path);
    let backend = waku_daemon::daemon::WakuBackend::new(settings, task_store)?;
    // Agent-scoped credentials dial back through this address; it is the
    // bound socket, not the `--bind` request.
    backend.set_daemon_address(address.to_string());
    let backend = Arc::new(backend);
    backend.install_weak_self();
    // Scheduled automations tick whether or not a client ever connects;
    // starting here is what makes them daemon-owned.
    backend.start_automations();
    // Binding the socket alone is not readiness: loading persisted state can
    // still fail. Owners may launch clients as soon as they receive this line.
    let ready = DaemonReady {
        address: address.to_string(),
        protocol_version: PROTOCOL_VERSION,
        pid: std::process::id(),
    };
    println!("{}", serde_json::to_string(&ready)?);
    std::io::stdout().flush()?;
    waku_daemon::serve(
        listener,
        token,
        backend.clone(),
        shutdown,
        waku_daemon::ServerOptions {
            allowed_origins: arguments.allowed_origins.into_iter().collect(),
            allow_shutdown: arguments.parent_pid.is_some(),
            build_commit: option_env!("GODDARD_COMMIT_SHA").map(str::to_owned),
        },
    )?;
    // Reached only on an orderly exit — a shutdown command or parent
    // death. The marker is how the next boot tells this from a jetsam
    // kill or a hard crash, which leave no chance to write one.
    backend.mark_clean_shutdown();
    Ok(())
}

/// Hold an exclusive advisory lock for the daemon's lifetime so a second
/// boot on this data directory cannot silently advance the shared database.
/// A normal handoff overlaps briefly while the previous owner drains through
/// its shutdown, so wait a bounded window — short enough to stay inside the
/// spawner's startup budget — then refuse, naming the owner pid. The lock
/// releases on process death; the file records the pid only to explain a wait
/// or refusal.
#[cfg(unix)]
fn lock_data_directory(
    directory: &std::path::Path,
) -> anyhow::Result<Option<std::fs::File>> {
    use std::os::fd::AsRawFd as _;
    use std::os::unix::fs::OpenOptionsExt as _;
    const WAIT: Duration = Duration::from_secs(10);
    const RETRY: Duration = Duration::from_millis(150);
    std::fs::create_dir_all(directory)?;
    let path = directory.join("daemon.lock");
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .custom_flags(libc::O_CLOEXEC)
        .open(&path)
        .context("could not open the Goddard data-directory lock")?;
    let deadline = std::time::Instant::now() + WAIT;
    let mut announced = false;
    loop {
        // SAFETY: flock on a valid fd; the returned File owns it until exit.
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
            break;
        }
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() != Some(libc::EWOULDBLOCK) {
            return Err(error).context("could not lock the Goddard data directory");
        }
        let owner = std::fs::read_to_string(&path).unwrap_or_default();
        let owner = owner.trim();
        if std::time::Instant::now() >= deadline {
            bail!(
                "another Goddard daemon already owns {}{}",
                directory.display(),
                if owner.is_empty() {
                    String::new()
                } else {
                    format!(" (pid {owner})")
                },
            );
        }
        if !announced {
            announced = true;
            eprintln!(
                "waiting for Goddard daemon{}to release {}",
                if owner.is_empty() {
                    " ".to_owned()
                } else {
                    format!(" pid {owner} ")
                },
                directory.display()
            );
        }
        std::thread::sleep(RETRY);
    }
    file.set_len(0)?;
    write!(file, "{}", std::process::id())?;
    file.sync_all()?;
    Ok(Some(file))
}

#[cfg(not(unix))]
fn lock_data_directory(
    _directory: &std::path::Path,
) -> anyhow::Result<Option<std::fs::File>> {
    Ok(None)
}

fn ensure_bind_allowed(address: SocketAddr, allow_non_loopback: bool) -> anyhow::Result<()> {
    if address.ip().is_loopback() || allow_non_loopback {
        return Ok(());
    }
    bail!(
        "refusing non-loopback daemon bind {address}; pass --allow-non-loopback only after configuring authentication and exact browser origins"
    )
}

struct Arguments {
    bind: String,
    parent_pid: Option<u32>,
    allowed_origins: Vec<String>,
    allow_non_loopback: bool,
}

impl Arguments {
    fn parse(arguments: impl IntoIterator<Item = String>) -> anyhow::Result<Self> {
        let mut bind = "127.0.0.1:0".to_owned();
        let mut parent_pid = None;
        let mut allowed_origins = Vec::new();
        let mut allow_non_loopback = false;
        let mut arguments = arguments.into_iter();
        while let Some(argument) = arguments.next() {
            match argument.as_str() {
                "--bind" => {
                    bind = arguments
                        .next()
                        .ok_or_else(|| anyhow!("--bind requires an address"))?;
                }
                "--parent-pid" => {
                    parent_pid = Some(
                        arguments
                            .next()
                            .ok_or_else(|| anyhow!("--parent-pid requires a process id"))?
                            .parse()
                            .context("--parent-pid is not a valid process id")?,
                    );
                }
                "--allow-origin" => {
                    let origin = arguments
                        .next()
                        .filter(|origin| !origin.trim().is_empty())
                        .ok_or_else(|| anyhow!("--allow-origin requires an origin"))?;
                    allowed_origins.push(origin);
                }
                "--allow-non-loopback" => {
                    allow_non_loopback = true;
                }
                "--help" | "-h" => {
                    println!(
                        "usage: {} [--bind ADDRESS] [--allow-non-loopback] [--parent-pid PID] [--allow-origin ORIGIN]...",
                        env!("CARGO_BIN_NAME")
                    );
                    std::process::exit(0);
                }
                unknown => bail!("unknown argument {unknown:?}"),
            }
        }
        Ok(Self {
            bind,
            parent_pid,
            allowed_origins,
            allow_non_loopback,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_loopback_listener_requires_an_explicit_flag() {
        assert!(ensure_bind_allowed("127.0.0.1:3000".parse().unwrap(), false).is_ok());
        assert!(ensure_bind_allowed("[::1]:3000".parse().unwrap(), false).is_ok());
        assert!(ensure_bind_allowed("0.0.0.0:3000".parse().unwrap(), false).is_err());
        assert!(ensure_bind_allowed("[::]:3000".parse().unwrap(), false).is_err());
        assert!(ensure_bind_allowed("0.0.0.0:3000".parse().unwrap(), true).is_ok());
    }

    #[test]
    fn parses_repeated_browser_origin_allowlist_entries() {
        let arguments = Arguments::parse([
            "--allow-origin".into(),
            "https://app.waku.test".into(),
            "--allow-origin".into(),
            "http://localhost:3000".into(),
        ])
        .unwrap();

        assert_eq!(
            arguments.allowed_origins,
            ["https://app.waku.test", "http://localhost:3000"]
        );
        assert!(!arguments.allow_non_loopback);
    }

    #[test]
    fn parses_explicit_non_loopback_opt_in() {
        let arguments = Arguments::parse(["--allow-non-loopback".into()]).unwrap();
        assert!(arguments.allow_non_loopback);
    }
}
