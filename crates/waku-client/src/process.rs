use std::io::{BufRead as _, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command as ProcessCommand, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::SystemTime;
use std::time::{Duration, Instant};

use anyhow::{Context as _, bail};
use crossbeam_channel::{Receiver, Sender, unbounded};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::DaemonClient;
use waku_protocol::{
    APP_EXECUTABLE_ENV, Command, DAEMON_TOKEN_ENV, DaemonReady, DaemonSettings, PROTOCOL_VERSION,
    ResponsePayload,
};
const START_TIMEOUT: Duration = Duration::from_secs(15);
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(1);
const REBUILD_POLL_INTERVAL: Duration = Duration::from_millis(500);
const PROBE_INTERVAL: Duration = Duration::from_secs(5);
const PROBE_TIMEOUT: Duration = Duration::from_secs(3);
const RETRY_BASE_DELAY: Duration = Duration::from_secs(1);
const RETRY_MAX_DELAY: Duration = Duration::from_secs(30);
const STABLE_UPTIME: Duration = Duration::from_secs(30);
const UNREACHABLE_AFTER_FAILURES: u32 = 4;
/// Reconnect attempts an alive-but-disconnected daemon earns before the
/// supervisor falls back to killing and respawning it.
const LOCAL_RECONNECT_ATTEMPTS: u32 = 3;
pub const DEFAULT_EXPOSED_DAEMON_PORT: u16 = 34_123;

/// Desktop-owned launch configuration for the daemon it supervises.
///
/// Provider settings belong to the daemon and live in `settings.json`; this
/// is an app preference because it controls how the desktop launches its own
/// child process. The bearer token is intentionally stable across daemon-only
/// rebuilds and desktop relaunches so a configured web client keeps working.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default)]
pub struct DaemonExposureSettings {
    pub enabled: bool,
    pub port: u16,
    pub allowed_origins: Vec<String>,
    pub token: String,
}

impl Default for DaemonExposureSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            port: DEFAULT_EXPOSED_DAEMON_PORT,
            allowed_origins: vec!["http://localhost:3001".into()],
            token: Self::new_token(),
        }
    }
}

impl DaemonExposureSettings {
    /// The bearer credential shown in daemon settings and typed into
    /// connecting clients — a mnemonic phrase, still just an exact-match
    /// string on the daemon. Older hex tokens keep working; nothing
    /// rewrites a stored one.
    pub fn new_token() -> String {
        crate::mnemonic::generate()
    }

    pub fn ensure_token(&mut self) -> bool {
        if !self.token.trim().is_empty() {
            return false;
        }
        self.token = Self::new_token();
        true
    }

    pub fn allowed_origins_text(&self) -> String {
        self.allowed_origins.join(", ")
    }

    pub fn with_allowed_origins_text(mut self, text: &str) -> anyhow::Result<Self> {
        self.allowed_origins = parse_allowed_origins(text)?;
        Ok(self)
    }

    pub fn validate(mut self) -> anyhow::Result<Self> {
        if self.port == 0 {
            bail!("daemon port must be between 1 and 65535");
        }
        if self.token.trim().is_empty() {
            bail!("daemon authentication token is empty");
        }
        self.allowed_origins = parse_allowed_origins(&self.allowed_origins_text())?;
        Ok(self)
    }

    /// The wire form sent with `setDaemonExposure` — `None` while disabled,
    /// since the daemon's exposed listener only exists while this is on.
    fn wire(&self) -> Option<waku_protocol::DaemonExposure> {
        self.enabled.then(|| waku_protocol::DaemonExposure {
            port: self.port,
            allowed_origins: self.allowed_origins.clone(),
            token: self.token.clone(),
        })
    }
}

pub use waku_protocol::parse_allowed_origins;

pub struct DaemonProcess {
    client: DaemonClient,
    child: Child,
    /// The bound address and bearer token — kept so a dropped connection on
    /// a live daemon can be re-established in place instead of respawning
    /// the process and orphaning every provider runtime.
    address: String,
    token: String,
}

impl DaemonProcess {
    pub fn spawn(executable: &Path) -> anyhow::Result<Self> {
        Self::spawn_configured(executable, DaemonExposureSettings::default())
    }

    fn spawn_configured(
        executable: &Path,
        settings: DaemonExposureSettings,
    ) -> anyhow::Result<Self> {
        let settings = settings.validate()?;
        let token = settings.token.clone();
        let app_executable =
            std::env::current_exe().context("could not locate Goddard executable")?;
        let mut command = ProcessCommand::new(executable);
        // The desktop is a GUI-subsystem binary on Windows, so a console
        // child would get a console window of its own. `stderr` still reaches
        // the app's inherited handle.
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt as _;

            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            command.creation_flags(CREATE_NO_WINDOW);
        }
        command
            // The managed daemon always binds loopback; exposure is applied
            // over the control socket afterwards so toggling it never
            // restarts the process.
            .arg("--bind")
            .arg("127.0.0.1:0")
            .arg("--parent-pid")
            .arg(std::process::id().to_string());
        for origin in &settings.allowed_origins {
            command.arg("--allow-origin").arg(origin);
        }
        let mut child = command
            .env(DAEMON_TOKEN_ENV, &token)
            .env(APP_EXECUTABLE_ENV, app_executable)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .with_context(|| format!("could not launch {}", executable.display()))?;
        let stdout = child
            .stdout
            .take()
            .context("Goddard daemon did not expose its readiness stream")?;
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name("goddard-daemon-ready".into())
            .spawn(move || {
                let mut line = String::new();
                let result = BufReader::new(stdout)
                    .read_line(&mut line)
                    .map_err(anyhow::Error::from)
                    .and_then(|bytes| {
                        if bytes == 0 {
                            bail!("Goddard daemon exited before becoming ready")
                        }
                        serde_json::from_str::<DaemonReady>(&line).map_err(anyhow::Error::from)
                    });
                let _ = ready_tx.send(result);
            })
            .context("could not start Goddard daemon readiness reader")?;
        let ready = match ready_rx.recv_timeout(START_TIMEOUT) {
            Ok(Ok(ready)) => ready,
            Ok(Err(error)) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error);
            }
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                bail!("timed out waiting for Goddard daemon: {error}");
            }
        };
        if ready.protocol_version != PROTOCOL_VERSION {
            let _ = child.kill();
            let _ = child.wait();
            bail!(
                "daemon protocol {} does not match desktop protocol {}",
                ready.protocol_version,
                PROTOCOL_VERSION
            );
        }
        let client_address = match desktop_client_address(&ready.address) {
            Ok(address) => address,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error);
            }
        };
        let client = match DaemonClient::connect(&client_address, token.clone()) {
            Ok(client) => client,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error);
            }
        };
        Ok(Self {
            client,
            child,
            address: client_address,
            token,
        })
    }

    pub fn client(&self) -> DaemonClient {
        self.client.clone()
    }

    /// Swap in a re-established connection on the same process.
    fn set_client(&mut self, client: DaemonClient) {
        self.client = client;
    }

    /// The exit status once the child has exited — `Some(None)` when it
    /// died but the status could not be read — and `None` while it still
    /// runs. A live daemon whose connection merely dropped reads `None`.
    fn exit_status(&mut self) -> Option<Option<std::process::ExitStatus>> {
        match self.child.try_wait() {
            Ok(status) => status.map(Some),
            Err(_) => Some(None),
        }
    }

    fn stop(&mut self) {
        self.client.shutdown();
        let deadline = Instant::now() + SHUTDOWN_TIMEOUT;
        while Instant::now() < deadline {
            match self.child.try_wait() {
                Ok(Some(_)) => return,
                Ok(None) => std::thread::sleep(Duration::from_millis(25)),
                Err(_) => break,
            }
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for DaemonProcess {
    fn drop(&mut self) {
        self.stop();
    }
}

fn desktop_client_address(address: &str) -> anyhow::Result<String> {
    let address = address
        .parse::<std::net::SocketAddr>()
        .with_context(|| format!("Goddard daemon returned an invalid address {address:?}"))?;
    let ip = if address.ip().is_unspecified() {
        if address.is_ipv4() {
            std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST)
        } else {
            std::net::IpAddr::V6(std::net::Ipv6Addr::LOCALHOST)
        }
    } else {
        address.ip()
    };
    Ok(std::net::SocketAddr::new(ip, address.port()).to_string())
}

/// The daemon address points back at this machine: a loopback or
/// unspecified IP, or a `localhost` name. Anything unparseable counts as
/// remote — a desktop PTY or reveal on a misread host is the worse error.
fn daemon_address_is_loopback(address: &str) -> bool {
    let normalized = if address.starts_with("ws://") || address.starts_with("wss://") {
        address.to_owned()
    } else {
        format!("ws://{address}")
    };
    let Ok(url) = url::Url::parse(&normalized) else {
        return false;
    };
    match url.host() {
        Some(url::Host::Ipv4(ip)) => ip.is_loopback() || ip.is_unspecified(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback() || ip.is_unspecified(),
        Some(url::Host::Domain(domain)) => {
            domain.eq_ignore_ascii_case("localhost") || domain.ends_with(".localhost")
        }
        None => false,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ExecutableStamp {
    modified: Option<SystemTime>,
    len: u64,
}

impl ExecutableStamp {
    fn read(path: &Path) -> anyhow::Result<Self> {
        let metadata = std::fs::metadata(path)
            .with_context(|| format!("could not inspect {}", path.display()))?;
        Ok(Self {
            modified: metadata.modified().ok(),
            len: metadata.len(),
        })
    }
}

/// The supervisor's view of daemon reachability. Callers use it to explain
/// failures (for example when a prompt cannot be delivered); it is not meant
/// to drive a persistent status surface.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DaemonStatus {
    /// The daemon answers requests.
    Connected,
    /// A restart or reconnect is under way.
    Recovering,
    /// Repeated recovery attempts have failed; retries continue slowly.
    Unreachable,
}

/// What made the supervisor recover its daemon connection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DaemonRecoveryCause {
    /// The managed daemon process exited.
    UnexpectedExit,
    /// The daemon's connection dropped while the process may still be
    /// running — a local daemon gets a reconnect attempt before any
    /// respawn, and sessions often survive this and reattach.
    Disconnect,
    /// The daemon binary changed on disk and was swapped (development).
    Rebuild,
}

/// How the managed daemon process ended, when the cause was a real exit:
/// its exit code, or the signal that killed it. `None` for connection-loss
/// and rebuild episodes, where no process exited at all.
#[derive(Clone, Copy, Debug)]
pub struct DaemonExit {
    pub code: Option<i32>,
    pub signal: Option<i32>,
}

impl DaemonExit {
    fn from_status(status: &std::process::ExitStatus) -> Self {
        #[cfg(unix)]
        let signal = {
            use std::os::unix::process::ExitStatusExt as _;
            status.signal()
        };
        #[cfg(not(unix))]
        let signal = None;
        Self {
            code: status.code(),
            signal,
        }
    }
}

/// How one recovery episode resolved.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DaemonRecoveryOutcome {
    /// A working daemon connection is back.
    Recovered,
    /// Recovery attempts crossed the unreachable threshold; slow retries
    /// continue, so a later report may still announce `Recovered`.
    Unreachable,
}

/// One daemon recovery episode, delivered to
/// [`DaemonSupervisor::subscribe_recovery`] subscribers: once when an outage
/// first crosses the unreachable threshold, and again when a working
/// connection is back.
#[derive(Clone, Copy, Debug)]
pub struct DaemonRecovery {
    pub cause: DaemonRecoveryCause,
    pub outcome: DaemonRecoveryOutcome,
    /// The managed process's exit detail — `Some` only when the daemon
    /// actually exited, which is what separates a crash from a dropped
    /// connection.
    pub exit: Option<DaemonExit>,
}

struct SupervisorInner {
    executable: Option<PathBuf>,
    /// The daemon's host is not this machine. An externally managed
    /// daemon on loopback is still local — desktop PTYs, reveals, and
    /// checkout work all target this host.
    remote: bool,
    target: Mutex<DaemonTarget>,
    exposure: Mutex<Option<DaemonExposureSettings>>,
    restart: Mutex<()>,
    settings: Mutex<DaemonSettings>,
    persisted_settings: Mutex<Option<DaemonSettings>>,
    settings_updates: Sender<DaemonSettings>,
    client_updates: Mutex<Vec<Sender<DaemonClient>>>,
    recovery_reports: Mutex<Vec<Sender<DaemonRecovery>>>,
    status: Mutex<DaemonStatus>,
    running: AtomicBool,
}

enum DaemonTarget {
    Local(DaemonProcess),
    Restarting(DaemonClient),
    Remote {
        client: DaemonClient,
        address: String,
        token: String,
    },
}

impl DaemonTarget {
    fn client(&self) -> DaemonClient {
        match self {
            Self::Local(process) => process.client(),
            Self::Restarting(client) => client.clone(),
            Self::Remote { client, .. } => client.clone(),
        }
    }
}

/// Owns the current daemon and, in development, swaps it after a successful
/// rebuild without requiring the desktop process to relaunch.
#[derive(Clone)]
pub struct DaemonSupervisor {
    inner: Arc<SupervisorInner>,
}

impl DaemonSupervisor {
    pub fn spawn(executable: &Path, watch_for_rebuilds: bool) -> anyhow::Result<Self> {
        Self::spawn_configured(
            executable,
            watch_for_rebuilds,
            DaemonExposureSettings::default(),
        )
    }

    pub fn spawn_configured(
        executable: &Path,
        watch_for_rebuilds: bool,
        exposure: DaemonExposureSettings,
    ) -> anyhow::Result<Self> {
        let exposure = exposure.validate()?;
        let process = DaemonProcess::spawn_configured(executable, exposure.clone())?;
        // A stuck exposure port must not keep the daemon itself down — the
        // local listener is the product, the exposed one is recoverable
        // through the next reconfigure.
        if let Err(error) = apply_daemon_exposure(&process.client(), &exposure) {
            eprintln!("could not expose the Goddard daemon: {error:#}");
        }
        let settings = read_settings(&process.client())?;
        let initial_stamp = ExecutableStamp::read(executable)?;
        let supervisor = Self::from_target(
            DaemonTarget::Local(process),
            Some(executable.to_owned()),
            false,
            Some(exposure),
            settings,
        )?;
        let weak_inner = Arc::downgrade(&supervisor.inner);
        std::thread::Builder::new()
            .name("goddard-daemon-supervisor".into())
            .spawn(move || monitor_daemon(weak_inner, Some(initial_stamp), watch_for_rebuilds))
            .context("could not start Goddard daemon supervisor")?;
        Ok(supervisor)
    }

    /// Connect to a daemon managed on another host (or by an external local
    /// service manager). Dropping the desktop never shuts this daemon down.
    pub fn connect(address: &str, token: String) -> anyhow::Result<Self> {
        let client = DaemonClient::connect(address, token.clone())?;
        let settings = read_settings(&client)?;
        let supervisor = Self::from_target(
            DaemonTarget::Remote {
                client,
                address: address.to_owned(),
                token,
            },
            None,
            !daemon_address_is_loopback(address),
            None,
            settings,
        )?;
        let weak_inner = Arc::downgrade(&supervisor.inner);
        std::thread::Builder::new()
            .name("waku-remote-daemon-supervisor".into())
            .spawn(move || monitor_daemon(weak_inner, None, false))
            .context("could not start remote Goddard daemon supervisor")?;
        Ok(supervisor)
    }

    fn from_target(
        target: DaemonTarget,
        executable: Option<PathBuf>,
        remote: bool,
        exposure: Option<DaemonExposureSettings>,
        settings: DaemonSettings,
    ) -> anyhow::Result<Self> {
        let (settings_updates, settings_update_rx) = unbounded();
        let inner = Arc::new(SupervisorInner {
            executable,
            remote,
            target: Mutex::new(target),
            exposure: Mutex::new(exposure),
            restart: Mutex::new(()),
            settings: Mutex::new(settings),
            // The desktop sends one normalized snapshot after it has migrated
            // the legacy combined settings document into app.json.
            persisted_settings: Mutex::new(None),
            settings_updates,
            client_updates: Mutex::new(Vec::new()),
            recovery_reports: Mutex::new(Vec::new()),
            status: Mutex::new(DaemonStatus::Connected),
            running: AtomicBool::new(true),
        });
        let weak_inner = Arc::downgrade(&inner);
        std::thread::Builder::new()
            .name("goddard-daemon-settings".into())
            .spawn(move || persist_settings(weak_inner, settings_update_rx))
            .context("could not start Goddard daemon settings writer")?;
        Ok(Self { inner })
    }

    pub fn client(&self) -> DaemonClient {
        self.inner.target.lock().client()
    }

    /// Subscribe to the active daemon connection. The current client is sent
    /// immediately, followed by each replacement after a managed restart.
    pub fn subscribe_clients(&self) -> Receiver<DaemonClient> {
        let (updates, receiver) = unbounded();
        // Holding the target lock through registration makes the initial send
        // atomic with respect to replacement: a subscriber sees either the old
        // client followed by the new one, or the new client directly.
        let target = self.inner.target.lock();
        self.inner.client_updates.lock().push(updates.clone());
        let _ = updates.send(target.client());
        receiver
    }

    /// Subscribe to daemon recovery episodes. Unlike [`Self::subscribe_clients`]
    /// there is no initial replay — a supervisor that has never had to recover
    /// has nothing to report.
    pub fn subscribe_recovery(&self) -> Receiver<DaemonRecovery> {
        let (reports, receiver) = unbounded();
        self.inner.recovery_reports.lock().push(reports);
        receiver
    }

    pub fn is_remote(&self) -> bool {
        self.inner.remote
    }

    /// The daemon's lifecycle is owned outside this app — a remote host or
    /// an external local manager — so settings cannot reconfigure or
    /// restart it.
    pub fn is_externally_managed(&self) -> bool {
        self.inner.executable.is_none()
    }

    /// The supervisor's current view of daemon reachability.
    pub fn status(&self) -> DaemonStatus {
        *self.inner.status.lock()
    }

    pub fn settings(&self) -> DaemonSettings {
        self.inner.settings.lock().clone()
    }

    /// Apply a new listener policy to the managed daemon in place — the
    /// daemon opens or closes its exposed socket itself, so no restart and
    /// no session interruption. The caller should run this off the UI
    /// thread.
    pub fn reconfigure(&self, exposure: DaemonExposureSettings) -> anyhow::Result<()> {
        let exposure = exposure.validate()?;
        if self.inner.executable.is_none() {
            bail!("the connected daemon is managed outside Goddard Desktop");
        }
        let _restart = self.inner.restart.lock();
        let client = self.inner.target.lock().client();
        set_daemon_exposure(&client, exposure.wire())?;
        *self.inner.exposure.lock() = Some(exposure);
        Ok(())
    }

    /// Queue a daemon settings update without blocking the desktop UI thread.
    pub fn update_settings(&self, settings: DaemonSettings) -> anyhow::Result<()> {
        *self.inner.settings.lock() = settings.clone();
        if self.inner.persisted_settings.lock().as_ref() == Some(&settings) {
            return Ok(());
        }
        self.inner
            .settings_updates
            .send(settings)
            .map_err(|_| anyhow::anyhow!("Goddard daemon settings writer is closed"))
    }

    /// Fold a `settingsChanged` broadcast into the supervisor's mirrors
    /// without re-sending it — the document is already persisted at the
    /// daemon, so the writer thread must not push it back.
    pub fn note_remote_settings(&self, settings: DaemonSettings) {
        *self.inner.settings.lock() = settings.clone();
        *self.inner.persisted_settings.lock() = Some(settings);
    }
}

impl Drop for DaemonSupervisor {
    fn drop(&mut self) {
        if Arc::strong_count(&self.inner) == 1 {
            self.inner.running.store(false, Ordering::Release);
        }
    }
}

/// Why the managed daemon needs recovery: a real process exit versus a
/// dead connection on a process that is still alive.
#[derive(Clone, Copy)]
enum LocalDown {
    /// The process exited; the status is `Some` when the OS reported one.
    Exited(Option<std::process::ExitStatus>),
    /// The process is alive but its connection is dead.
    Disconnected,
}

fn monitor_daemon(
    weak_inner: std::sync::Weak<SupervisorInner>,
    mut active_stamp: Option<ExecutableStamp>,
    watch_for_rebuilds: bool,
) {
    let mut last_probe = Instant::now();
    let mut healthy_since = Instant::now();
    let mut consecutive_failures = 0_u32;
    let mut next_retry = Instant::now();
    // The open episode's cause and exit detail, captured at first
    // detection — mid-respawn the target reads `Restarting`, which has no
    // process to inspect, so later iterations would report the wrong cause.
    let mut outage: Option<(DaemonRecoveryCause, Option<DaemonExit>)> = None;
    loop {
        std::thread::sleep(REBUILD_POLL_INTERVAL);
        let Some(inner) = weak_inner.upgrade() else {
            return;
        };
        if !inner.running.load(Ordering::Acquire) {
            return;
        }
        let remote_reconnect = {
            let target = inner.target.lock();
            match &*target {
                DaemonTarget::Remote {
                    client,
                    address,
                    token,
                } if client.is_disconnected() => Some((
                    client.clone(),
                    address.clone(),
                    token.clone(),
                    client.last_sequences(),
                )),
                _ => None,
            }
        };
        if let Some((disconnected, address, token, resume_from)) = remote_reconnect {
            if Instant::now() < next_retry {
                continue;
            }
            let _restart = inner.restart.lock();
            let still_current = matches!(
                &*inner.target.lock(),
                DaemonTarget::Remote { client, .. }
                    if client.same_connection(&disconnected) && client.is_disconnected()
            );
            if !still_current {
                continue;
            }
            set_status(&inner, DaemonStatus::Recovering);
            match DaemonClient::connect_with_resume(&address, token.clone(), resume_from) {
                Ok(replacement) => {
                    *inner.target.lock() = DaemonTarget::Remote {
                        client: replacement.clone(),
                        address,
                        token,
                    };
                    inner
                        .client_updates
                        .lock()
                        .retain(|subscriber| subscriber.send(replacement.clone()).is_ok());
                    consecutive_failures = 0;
                    healthy_since = Instant::now();
                    mark_connected(&inner, DaemonRecoveryCause::Disconnect, None);
                }
                Err(error) => {
                    consecutive_failures += 1;
                    next_retry = Instant::now() + retry_delay(consecutive_failures);
                    eprintln!("could not reconnect to Goddard daemon: {error:#}");
                    note_recovery_failure(
                        &inner,
                        consecutive_failures,
                        DaemonRecoveryCause::Disconnect,
                        None,
                    );
                }
            }
            continue;
        }
        let (down, client) = {
            let mut target = inner.target.lock();
            match &mut *target {
                DaemonTarget::Local(process) => (local_down(process), process.client()),
                DaemonTarget::Restarting(client) => {
                    (Some(LocalDown::Exited(None)), client.clone())
                }
                DaemonTarget::Remote { client, .. } => (None, client.clone()),
            }
        };
        if let Some(executable) = inner.executable.as_ref() {
            let observed_stamp = ExecutableStamp::read(executable).ok();
            let executable_changed = watch_for_rebuilds
                && observed_stamp.is_some_and(|observed| Some(observed) != active_stamp);
            if down.is_some() || executable_changed {
                if Instant::now() < next_retry {
                    continue;
                }
                set_status(&inner, DaemonStatus::Recovering);
                let _restart = inner.restart.lock();
                // Re-check under the restart lock: `reconfigure` may have
                // swapped in a fresh daemon while this thread waited.
                let still_down = match &mut *inner.target.lock() {
                    DaemonTarget::Local(process) => local_down(process),
                    DaemonTarget::Restarting(_) => Some(LocalDown::Exited(None)),
                    DaemonTarget::Remote { .. } => None,
                };
                // A downed daemon owns the cause even when a rebuild is also
                // pending: the process exit is what interrupted sessions.
                let (cause, exit) = match still_down {
                    Some(LocalDown::Exited(status)) => (
                        DaemonRecoveryCause::UnexpectedExit,
                        Some(status.map_or(
                            DaemonExit {
                                code: None,
                                signal: None,
                            },
                            |status| DaemonExit::from_status(&status),
                        )),
                    ),
                    Some(LocalDown::Disconnected) => (DaemonRecoveryCause::Disconnect, None),
                    None => (DaemonRecoveryCause::Rebuild, None),
                };
                if still_down.is_none() && !executable_changed {
                    mark_connected(&inner, cause, exit);
                    continue;
                }
                if still_down.is_some() && outage.is_none() {
                    // Pin the episode's cause and exit detail at first
                    // detection — respawn retries observe `Restarting`, not
                    // the dead process.
                    outage = Some((cause, exit));
                    // A daemon that dies within STABLE_UPTIME of its own
                    // launch is crash-looping; an older one had a stable run
                    // and gets an immediate replacement.
                    if healthy_since.elapsed() < STABLE_UPTIME {
                        consecutive_failures += 1;
                    }
                    if consecutive_failures > 0 {
                        next_retry = Instant::now() + retry_delay(consecutive_failures);
                        note_recovery_failure(&inner, consecutive_failures, cause, exit);
                        continue;
                    }
                }
                let (cause, exit) = outage.unwrap_or((cause, exit));
                // A daemon whose process is still alive only lost its
                // connection — reconnect in place; killing it would take
                // every provider runtime down for nothing.
                if matches!(still_down, Some(LocalDown::Disconnected))
                    && consecutive_failures < LOCAL_RECONNECT_ATTEMPTS
                {
                    match reconnect_local_daemon(&inner) {
                        Ok(()) => {
                            consecutive_failures = 0;
                            healthy_since = Instant::now();
                            mark_connected(&inner, cause, exit);
                        }
                        Err(error) => {
                            consecutive_failures += 1;
                            next_retry = Instant::now() + retry_delay(consecutive_failures);
                            eprintln!("could not reconnect to the Goddard daemon: {error:#}");
                            note_recovery_failure(&inner, consecutive_failures, cause, exit);
                        }
                    }
                    continue;
                }
                let Some(exposure) = inner.exposure.lock().clone() else {
                    return;
                };
                match replace_local_daemon(&inner, executable, &exposure) {
                    Ok(()) => {
                        healthy_since = Instant::now();
                        mark_connected(&inner, cause, exit);
                        queue_settings_refresh(&inner);
                        if let Some(observed_stamp) = observed_stamp {
                            active_stamp = Some(observed_stamp);
                        }
                    }
                    Err(error) => {
                        consecutive_failures += 1;
                        next_retry = Instant::now() + retry_delay(consecutive_failures);
                        eprintln!("could not restart the Goddard daemon: {error:#}");
                        note_recovery_failure(&inner, consecutive_failures, cause, exit);
                    }
                }
                continue;
            }
            outage = None;
            if consecutive_failures > 0 && healthy_since.elapsed() > STABLE_UPTIME {
                consecutive_failures = 0;
                set_status(&inner, DaemonStatus::Connected);
            }
        }
        // The socket being open only proves the socket is open. Probe the
        // request pipeline so a wedged daemon is declared dead and replaced
        // instead of hanging every request for the full request timeout.
        if last_probe.elapsed() >= PROBE_INTERVAL && !client.is_disconnected() {
            last_probe = Instant::now();
            if !client.probe(PROBE_TIMEOUT) {
                client.force_disconnect();
            }
        }
    }
}

fn retry_delay(failures: u32) -> Duration {
    let shift = failures.saturating_sub(1).min(5);
    (RETRY_BASE_DELAY * 2_u32.pow(shift)).min(RETRY_MAX_DELAY)
}

fn set_status(inner: &SupervisorInner, status: DaemonStatus) {
    *inner.status.lock() = status;
}

/// The managed daemon's down state, if any — a real exit beats a dead
/// connection because the exit is what interrupted sessions.
fn local_down(process: &mut DaemonProcess) -> Option<LocalDown> {
    match process.exit_status() {
        Some(status) => Some(LocalDown::Exited(status)),
        None if process.client().is_disconnected() => Some(LocalDown::Disconnected),
        None => None,
    }
}

fn report_recovery(
    inner: &SupervisorInner,
    cause: DaemonRecoveryCause,
    outcome: DaemonRecoveryOutcome,
    exit: Option<DaemonExit>,
) {
    inner.recovery_reports.lock().retain(|subscriber| {
        subscriber
            .send(DaemonRecovery {
                cause,
                outcome,
                exit,
            })
            .is_ok()
    });
}

/// Recovery succeeded — report it only when the daemon was actually
/// recovering, so steady-state bookkeeping never reads as an outage.
fn mark_connected(
    inner: &SupervisorInner,
    cause: DaemonRecoveryCause,
    exit: Option<DaemonExit>,
) {
    let recovered = {
        let mut status = inner.status.lock();
        let recovered = *status != DaemonStatus::Connected;
        *status = DaemonStatus::Connected;
        recovered
    };
    if recovered {
        report_recovery(inner, cause, DaemonRecoveryOutcome::Recovered, exit);
    }
}

fn note_recovery_failure(
    inner: &SupervisorInner,
    failures: u32,
    cause: DaemonRecoveryCause,
    exit: Option<DaemonExit>,
) {
    let newly_unreachable = {
        let mut status = inner.status.lock();
        let unreachable = failures >= UNREACHABLE_AFTER_FAILURES;
        let newly = unreachable && *status != DaemonStatus::Unreachable;
        *status = if unreachable {
            DaemonStatus::Unreachable
        } else {
            DaemonStatus::Recovering
        };
        newly
    };
    // Report the crossing once per episode — slow retries keep calling this
    // and would otherwise repeat the same report.
    if newly_unreachable {
        report_recovery(inner, cause, DaemonRecoveryOutcome::Unreachable, exit);
    }
}

/// The managed daemon's connection died but the process is alive: open a
/// fresh session-bearing connection in place instead of killing it and
/// every provider runtime with it. Sessions keep their runtimes — the new
/// client resumes replay from the dead one's cursors.
fn reconnect_local_daemon(inner: &SupervisorInner) -> anyhow::Result<()> {
    let (address, token, resume_from, expected) = {
        let target = inner.target.lock();
        match &*target {
            DaemonTarget::Local(process) if process.client().is_disconnected() => (
                process.address.clone(),
                process.token.clone(),
                process.client().last_sequences(),
                process.client(),
            ),
            _ => bail!("the daemon no longer needs reconnecting"),
        }
    };
    let client = DaemonClient::connect_with_resume(&address, token, resume_from)?;
    let mut target = inner.target.lock();
    let DaemonTarget::Local(process) = &mut *target else {
        bail!("the daemon changed while reconnecting");
    };
    if !process.client().same_connection(&expected) {
        bail!("the daemon changed while reconnecting");
    }
    process.set_client(client.clone());
    inner
        .client_updates
        .lock()
        .retain(|subscriber| subscriber.send(client.clone()).is_ok());
    Ok(())
}

fn replace_local_daemon(
    inner: &SupervisorInner,
    executable: &Path,
    exposure: &DaemonExposureSettings,
) -> anyhow::Result<()> {
    let previous = {
        let mut target = inner.target.lock();
        match &*target {
            DaemonTarget::Remote { .. } => {
                bail!("the connected daemon is managed outside Goddard Desktop")
            }
            DaemonTarget::Restarting(_) => None,
            DaemonTarget::Local(process) => {
                let disconnected = process.client();
                let previous =
                    std::mem::replace(&mut *target, DaemonTarget::Restarting(disconnected));
                match previous {
                    DaemonTarget::Local(process) => Some(process),
                    _ => unreachable!("local daemon target changed while locked"),
                }
            }
        }
    };
    // Dropping can wait briefly for graceful shutdown, but the target lock is
    // already released so UI actions never block behind process teardown.
    drop(previous);
    let replacement = DaemonProcess::spawn_configured(executable, exposure.clone())?;
    if let Err(error) = apply_daemon_exposure(&replacement.client(), exposure) {
        eprintln!("could not expose the Goddard daemon: {error:#}");
    }
    let client = replacement.client();
    *inner.target.lock() = DaemonTarget::Local(replacement);
    inner
        .client_updates
        .lock()
        .retain(|subscriber| subscriber.send(client.clone()).is_ok());
    Ok(())
}

/// Push the exposure half of a launch configuration onto a freshly spawned
/// daemon. Disabled means the daemon keeps its startup loopback-only
/// listener, so there is nothing to send.
fn apply_daemon_exposure(
    client: &DaemonClient,
    exposure: &DaemonExposureSettings,
) -> anyhow::Result<()> {
    if !exposure.enabled {
        return Ok(());
    }
    set_daemon_exposure(client, exposure.wire())
}

/// Send `setDaemonExposure` and verify the daemon understood it.
fn set_daemon_exposure(
    client: &DaemonClient,
    exposure: Option<waku_protocol::DaemonExposure>,
) -> anyhow::Result<()> {
    match client.request(
        Uuid::nil(),
        Uuid::nil(),
        Command::SetDaemonExposure { exposure },
    )? {
        ResponsePayload::Exposure { .. } => Ok(()),
        _ => bail!("Goddard daemon returned an invalid exposure response"),
    }
}

fn queue_settings_refresh(inner: &SupervisorInner) {
    let settings = inner.settings.lock().clone();
    *inner.persisted_settings.lock() = None;
    let _ = inner.settings_updates.send(settings);
}

fn read_settings(client: &DaemonClient) -> anyhow::Result<DaemonSettings> {
    match client.request(Uuid::nil(), Uuid::nil(), Command::GetSettings)? {
        ResponsePayload::Settings { settings } => Ok(settings),
        _ => bail!("Goddard daemon returned an invalid settings response"),
    }
}

fn persist_settings(
    weak_inner: std::sync::Weak<SupervisorInner>,
    updates: Receiver<DaemonSettings>,
) {
    while let Ok(mut settings) = updates.recv() {
        while let Ok(newer) = updates.try_recv() {
            settings = newer;
        }
        loop {
            let Some(inner) = weak_inner.upgrade() else {
                return;
            };
            if !inner.running.load(Ordering::Acquire) {
                return;
            }
            let desired = inner.settings.lock().clone();
            if desired != settings {
                settings = desired;
            }
            let client = inner.target.lock().client();
            let result = client.request(
                Uuid::nil(),
                Uuid::nil(),
                Command::UpdateSettings {
                    settings: settings.clone(),
                },
            );
            match result {
                Ok(ResponsePayload::Ack) => {
                    *inner.persisted_settings.lock() = Some(settings);
                    break;
                }
                Ok(_) => {
                    eprintln!("Goddard daemon returned an invalid settings update response");
                }
                Err(error) => {
                    eprintln!("could not persist Goddard daemon settings: {error:#}");
                }
            }
            drop(inner);
            std::thread::sleep(REBUILD_POLL_INTERVAL);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browser_origins_are_exact_and_deduplicated() {
        assert_eq!(
            parse_allowed_origins(
                "https://app.waku.test, http://localhost:3001, https://app.waku.test"
            )
            .unwrap(),
            ["https://app.waku.test", "http://localhost:3001"]
        );
        assert!(parse_allowed_origins("https://app.waku.test/path").is_err());
        assert!(parse_allowed_origins("ws://app.waku.test").is_err());
    }

    #[test]
    fn desktop_uses_loopback_to_reach_an_unspecified_listener() {
        assert_eq!(
            desktop_client_address("0.0.0.0:34123").unwrap(),
            "127.0.0.1:34123"
        );
        assert_eq!(desktop_client_address("[::]:34123").unwrap(), "[::1]:34123");
    }

    #[test]
    fn loopback_addresses_are_not_remote() {
        for local in [
            "127.0.0.1:34123",
            "127.0.0.2:34123",
            "localhost:34123",
            "dev.localhost:34123",
            "[::1]:34123",
            "0.0.0.0:34123",
            "ws://127.0.0.1:34123",
            "ws://localhost:34123/v1",
        ] {
            assert!(daemon_address_is_loopback(local), "{local}");
        }
        for remote in [
            "10.0.0.5:34123",
            "ws://daemon.example.com",
            "not an address",
        ] {
            assert!(!daemon_address_is_loopback(remote), "{remote}");
        }
    }
}
