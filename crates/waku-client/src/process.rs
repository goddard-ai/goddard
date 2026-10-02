use std::fs::{self, OpenOptions};
use std::io::{BufRead as _, BufReader, Read, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStderr, Command as ProcessCommand, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};
use std::time::{SystemTime, UNIX_EPOCH};

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
/// The whole startup budget — process spawn, the daemon's ready line, the
/// control-socket connect, and the first settings read share it — so a
/// machine too contended to boot a daemon fails one bounded attempt instead
/// of hanging mid-phase.
const START_TIMEOUT: Duration = Duration::from_secs(15);
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(1);
const REBUILD_POLL_INTERVAL: Duration = Duration::from_millis(500);
const PROBE_INTERVAL: Duration = Duration::from_secs(5);
const PROBE_TIMEOUT: Duration = Duration::from_secs(3);
/// A reconnect or second-opinion probe gets this long to reach a live
/// daemon — longer than a probe answer itself, short enough that the next
/// retry starts promptly on a genuinely dead endpoint.
const CONNECT_ATTEMPT_TIMEOUT: Duration = Duration::from_secs(10);
const RETRY_BASE_DELAY: Duration = Duration::from_secs(1);
const RETRY_MAX_DELAY: Duration = Duration::from_secs(30);
const STABLE_UPTIME: Duration = Duration::from_secs(30);
const UNREACHABLE_AFTER_FAILURES: u32 = 4;
const DAEMON_STDERR_LOG_CAP: u64 = 256 * 1024;
const MAX_DAEMON_STDERR_LINE_BYTES: usize = 8 * 1024;
static DAEMON_STDERR_LOG_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
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
        Self::spawn_configured(
            executable,
            DaemonExposureSettings::default(),
            Instant::now() + START_TIMEOUT,
        )
    }

    /// Spawn under a caller-owned deadline: the child spawn, the daemon's
    /// ready line, and the control-socket connect share `deadline`, so one
    /// failed attempt costs at most the budget and always reaps the child.
    fn spawn_configured(
        executable: &Path,
        settings: DaemonExposureSettings,
        deadline: Instant,
    ) -> anyhow::Result<Self> {
        let settings = settings.validate()?;
        let token = settings.token.clone();
        let app_executable =
            std::env::current_exe().context("could not locate Goddard executable")?;
        // Create the reader before spawning the daemon so failure to allocate
        // a drain thread falls back to inherited stderr instead of leaving a
        // child blocked on a full pipe.
        let stderr_path = dirs::home_dir().map(|home| home.join(".goddard/daemon-stderr.jsonl"));
        let (stderr_sender, capture_stderr) = {
            let (sender, receiver) = mpsc::sync_channel(1);
            let reader = std::thread::Builder::new()
                .name("goddard-daemon-stderr".into())
                .spawn(move || {
                    if let Ok(stderr) = receiver.recv() {
                        pump_daemon_stderr(stderr, stderr_path);
                    }
                });
            match reader {
                Ok(_) => (Some(sender), true),
                Err(_) => (None, false),
            }
        };
        let mut command = ProcessCommand::new(executable);
        // The desktop is a GUI-subsystem binary on Windows, so a console
        // child would get a console window of its own. Captured stderr is
        // forwarded through a background reader to the inherited handle.
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
            .stderr(if capture_stderr {
                Stdio::piped()
            } else {
                Stdio::inherit()
            })
            .spawn()
            .with_context(|| format!("could not launch {}", executable.display()))?;
        if let Some(sender) = stderr_sender {
            let Some(stderr) = child.stderr.take() else {
                let _ = child.kill();
                let _ = child.wait();
                bail!("Goddard daemon did not expose its stderr stream");
            };
            if sender.send(stderr).is_err() {
                let _ = child.kill();
                let _ = child.wait();
                bail!("could not start the Goddard daemon stderr reader");
            }
        }
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
        let ready = match ready_rx.recv_timeout(remaining_budget(deadline)) {
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
        let client = match DaemonClient::connect_before(
            &client_address,
            token.clone(),
            Vec::new(),
            deadline,
        ) {
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

fn pump_daemon_stderr(mut stderr: ChildStderr, path: Option<PathBuf>) {
    let mut buffer = [0u8; 4096];
    let mut line = Vec::with_capacity(512);
    let mut too_long = false;
    loop {
        let bytes = match stderr.read(&mut buffer) {
            Ok(0) => break,
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        };
        for byte in buffer[..bytes].iter().copied() {
            if byte == b'\n' {
                if !too_long {
                    emit_daemon_stderr_line(&line, false, path.as_deref());
                }
                line.clear();
                too_long = false;
            } else if too_long {
                continue;
            } else if line.len() < MAX_DAEMON_STDERR_LINE_BYTES {
                line.push(byte);
            } else {
                emit_daemon_stderr_line(&[], true, path.as_deref());
                line.clear();
                too_long = true;
            }
        }
    }
    if !line.is_empty() && !too_long {
        emit_daemon_stderr_line(&line, false, path.as_deref());
    }
}

fn emit_daemon_stderr_line(raw: &[u8], too_long: bool, path: Option<&Path>) {
    let line = if too_long {
        "[daemon stderr line omitted: exceeded the 8 KiB limit]".to_owned()
    } else {
        String::from_utf8_lossy(raw)
            .trim_end_matches('\r')
            .to_owned()
    };
    if line.is_empty() {
        return;
    }
    let line = redact_daemon_stderr(&line);
    if let Some(path) = path {
        let _ = append_capped_daemon_stderr(path, &line);
    }
    let mut stderr = std::io::stderr().lock();
    let _ = writeln!(stderr, "{line}");
}

fn redact_daemon_stderr(input: &str) -> String {
    const SECRET_KEYS: [&str; 19] = [
        "refresh-token",
        "refresh_token",
        "access-token",
        "access_token",
        "client-secret",
        "client_secret",
        "private-key",
        "private_key",
        "api_key",
        "api-key",
        "apikey",
        "authorization",
        "password",
        "passwd",
        "secret",
        "api key",
        "cookie",
        "credential",
        "token",
    ];

    let input = redact_url_userinfo(input);
    let lowercase = input.to_ascii_lowercase();
    let bytes = input.as_bytes();
    let mut output = String::with_capacity(input.len());
    let mut cursor = 0;
    while cursor < input.len() {
        if let Some((value_start, value_end)) = bearer_value_range(bytes, &lowercase, cursor) {
            output.push_str(&input[cursor..value_start]);
            output.push_str("[REDACTED]");
            cursor = value_end;
            continue;
        }
        if let Some((value_start, value_end)) =
            secret_value_range(bytes, &lowercase, cursor, &SECRET_KEYS)
        {
            output.push_str(&input[cursor..value_start]);
            output.push_str("[REDACTED]");
            cursor = value_end;
            continue;
        }
        let character = input[cursor..]
            .chars()
            .next()
            .expect("cursor is before the end of the string");
        output.push(character);
        cursor += character.len_utf8();
    }

    if let Some(home) = dirs::home_dir().and_then(|path| path.into_os_string().into_string().ok())
        && !home.is_empty()
    {
        output = output.replace(&home, "~");
    }
    output
}

fn bearer_value_range(bytes: &[u8], lowercase: &str, start: usize) -> Option<(usize, usize)> {
    if start > 0 && bytes[start - 1].is_ascii_alphanumeric() {
        return None;
    }
    if !lowercase.get(start..)?.starts_with("bearer") {
        return None;
    }
    let mut value_start = start + "bearer".len();
    if !bytes.get(value_start).is_some_and(u8::is_ascii_whitespace) {
        return None;
    }
    while bytes.get(value_start).is_some_and(u8::is_ascii_whitespace) {
        value_start += 1;
    }
    let mut value_end = value_start;
    while let Some(byte) = bytes.get(value_end) {
        if byte.is_ascii_whitespace() || matches!(*byte, b'"' | b'\'' | b',' | b';') {
            break;
        }
        value_end += 1;
    }
    (value_end > value_start).then_some((value_start, value_end))
}

fn secret_value_range(
    bytes: &[u8],
    lowercase: &str,
    start: usize,
    keys: &[&str],
) -> Option<(usize, usize)> {
    if start > 0 && bytes[start - 1].is_ascii_alphanumeric() {
        let camel_case_boundary = bytes[start - 1].is_ascii_lowercase()
            && bytes.get(start).is_some_and(u8::is_ascii_uppercase);
        if !camel_case_boundary {
            return None;
        }
    }
    let key = keys.iter().find(|key| {
        lowercase
            .get(start..)
            .is_some_and(|tail| tail.starts_with(**key))
    })?;
    let key_end = start + key.len();
    if bytes
        .get(key_end)
        .is_some_and(|byte| byte.is_ascii_alphanumeric() || matches!(*byte, b'_' | b'-'))
    {
        return None;
    }

    let mut separator = key_end;
    if bytes
        .get(separator)
        .is_some_and(|byte| matches!(*byte, b'"' | b'\'') && start > 0 && bytes[start - 1] == *byte)
    {
        separator += 1;
    }
    let mut value_start = separator;
    let mut had_space = false;
    while bytes.get(value_start).is_some_and(u8::is_ascii_whitespace) {
        had_space = true;
        value_start += 1;
    }
    if bytes
        .get(value_start)
        .is_some_and(|byte| matches!(*byte, b':' | b'='))
    {
        value_start += 1;
        while bytes.get(value_start).is_some_and(u8::is_ascii_whitespace) {
            value_start += 1;
        }
    } else if had_space && start >= 2 && &bytes[start - 2..start] == b"--" {
        // `--token value` and similar CLI options.
    } else {
        return None;
    }

    let Some(first) = bytes.get(value_start).copied() else {
        return None;
    };
    if matches!(first, b'"' | b'\'') {
        let quote = first;
        let content_start = value_start + 1;
        let mut end = content_start;
        while end < bytes.len() {
            if bytes[end] == quote && (end == content_start || bytes[end - 1] != b'\\') {
                return (end > content_start).then_some((content_start, end));
            }
            end += 1;
        }
        return (content_start < bytes.len()).then_some((content_start, bytes.len()));
    }

    let whole_field = *key == "authorization" || *key == "cookie";
    let mut value_end = value_start;
    while let Some(byte) = bytes.get(value_end) {
        let delimiter = if whole_field {
            matches!(*byte, b',' | b';' | b'}' | b']' | b'"' | b'\'')
        } else {
            byte.is_ascii_whitespace() || matches!(*byte, b',' | b';' | b'}' | b']' | b'"' | b'\'')
        };
        if delimiter {
            break;
        }
        value_end += 1;
    }
    (value_end > value_start).then_some((value_start, value_end))
}

fn redact_url_userinfo(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut cursor = 0;
    while let Some(scheme) = input.get(cursor..).and_then(|tail| tail.find("://")) {
        let authority_start = cursor + scheme + 3;
        let authority_end = input[authority_start..]
            .find(|character: char| matches!(character, '/' | '?' | '#' | ' ' | '\t' | '\r' | '\n'))
            .map_or(input.len(), |offset| authority_start + offset);
        let authority = &input[authority_start..authority_end];
        if let Some(user_info_end) = authority.rfind('@') {
            output.push_str(&input[cursor..authority_start]);
            output.push_str("[REDACTED]@");
            cursor = authority_start + user_info_end + 1;
        } else {
            output.push_str(&input[cursor..authority_end]);
            cursor = authority_end;
        }
    }
    output.push_str(&input[cursor..]);
    output
}

fn append_capped_daemon_stderr(path: &Path, line: &str) -> std::io::Result<()> {
    let _guard = DAEMON_STDERR_LOG_LOCK.get_or_init(|| Mutex::new(())).lock();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let at_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let record = serde_json::json!({ "atMs": at_ms, "line": line });
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(file, "{record}")?;
    drop(file);
    if fs::metadata(path)
        .map(|metadata| metadata.len())
        .unwrap_or(0)
        <= DAEMON_STDERR_LOG_CAP
    {
        return Ok(());
    }
    let bytes = fs::read(path)?;
    let halfway = bytes.len() / 2;
    let start = bytes[halfway..]
        .iter()
        .position(|byte| *byte == b'\n')
        .map(|offset| halfway + offset + 1)
        .unwrap_or(bytes.len());
    fs::write(path, &bytes[start..])
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
    /// A managed local daemon that stays alive while its connection stays
    /// dead — the app keeps trying to reconnect in place, and only an
    /// explicit restart or a confirmed exit replaces it.
    Degraded,
    /// Repeated recovery attempts have failed; retries continue slowly.
    /// A daemon whose process is unreachable — dead, respawn-failing, or
    /// managed on another host.
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
    /// What the supervisor first observed take the daemon out. An episode
    /// that began as a dropped connection keeps `Disconnect` even if the
    /// process later exits or is replaced — [`Self::exit`] and
    /// [`Self::replaced`] carry what happened next.
    pub cause: DaemonRecoveryCause,
    pub outcome: DaemonRecoveryOutcome,
    /// The latest confirmed process exit observed during the episode —
    /// `Some` even for episodes that began as connection loss when the
    /// daemon subsequently died, `None` when no process ever exited.
    pub exit: Option<DaemonExit>,
    /// The recovered connection runs against a freshly spawned daemon —
    /// `false` means the same process was reconnected in place and its
    /// provider runtimes survived. Always `false` for daemons this app
    /// does not manage.
    pub replaced: bool,
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
    /// Every status transition — subscribers get the current status at
    /// subscribe time, then each change as it happens.
    status_updates: Mutex<Vec<Sender<DaemonStatus>>>,
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
        // One deadline covers the daemon's whole startup: process launch,
        // its ready line, the control-socket connect, and the settings
        // read. Every phase shares it so a contended boot fails bounded
        // instead of stretching per phase.
        let startup_deadline = Instant::now() + START_TIMEOUT;
        let process =
            DaemonProcess::spawn_configured(executable, exposure.clone(), startup_deadline)?;
        // A stuck exposure port must not keep the daemon itself down — the
        // local listener is the product, the exposed one is recoverable
        // through the next reconfigure.
        if let Err(error) =
            apply_daemon_exposure(&process.client(), &exposure, startup_deadline)
        {
            eprintln!("could not expose the Goddard daemon: {error:#}");
        }
        let settings = read_settings(&process.client(), remaining_budget(startup_deadline))?;
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
        let deadline = Instant::now() + CONNECT_ATTEMPT_TIMEOUT;
        let client =
            DaemonClient::connect_before(address, token.clone(), Vec::new(), deadline)?;
        let settings = read_settings(&client, remaining_budget(deadline))?;
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
            status_updates: Mutex::new(Vec::new()),
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

    /// Subscribe to daemon status transitions. The current status is sent
    /// immediately, then each change — a degraded daemon stays announced
    /// until a working connection is back.
    pub fn subscribe_status(&self) -> Receiver<DaemonStatus> {
        let (updates, receiver) = unbounded();
        let _ = updates.send(self.status());
        self.inner.status_updates.lock().push(updates);
        receiver
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
        set_daemon_exposure(&client, exposure.wire(), CONNECT_ATTEMPT_TIMEOUT)?;
        *self.inner.exposure.lock() = Some(exposure);
        Ok(())
    }

    /// The user asked for the local daemon to be replaced — the only path
    /// besides a confirmed process exit that kills a live daemon. Runs the
    /// whole swap on the caller's thread (the monitor serializes through
    /// the same restart lock); callers dispatch off the UI thread.
    pub fn restart_local_daemon(&self) -> anyhow::Result<()> {
        let Some(executable) = self.inner.executable.clone() else {
            bail!("the connected daemon is managed outside Goddard Desktop");
        };
        let Some(exposure) = self.inner.exposure.lock().clone() else {
            bail!("the desktop does not manage this daemon's settings");
        };
        let _restart = self.inner.restart.lock();
        // `Restarting` can't be observed here — holding the restart lock
        // means no swap is in flight; whatever the target is gets replaced.
        replace_local_daemon(&self.inner, &executable, &exposure)
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
    /// The slot is deliberately empty while a replacement spawns — a
    /// supervisor action, not a fresh observation about the process.
    Respawning,
}

/// The monitor's record of one continuous outage. The episode opens on the
/// first observed failure and closes when a working connection is back, so
/// a stretch of retries reports one unreachable transition and one
/// recovery — never a report per attempt.
struct RecoveryEpisode {
    /// What the supervisor first observed — a process exit, a dropped
    /// connection, or a rebuild swap. Pinned for the episode so the first
    /// failure's meaning survives later transitions.
    cause: DaemonRecoveryCause,
    /// The most recent confirmed process exit — an episode that opened on
    /// a disconnect can still end in a real exit during respawn attempts.
    exit: Option<DaemonExit>,
    /// A spawned daemon took the downed one's place.
    replaced: bool,
    /// The unreachable crossing was already announced this episode.
    unreachable_announced: bool,
}

impl RecoveryEpisode {
    fn new(cause: DaemonRecoveryCause, exit: Option<DaemonExit>) -> Self {
        Self {
            cause,
            exit,
            replaced: false,
            unreachable_announced: false,
        }
    }
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
    // The open episode, captured at first failure detection — mid-respawn
    // the target reads `Restarting`, which has no process to inspect, so
    // later iterations would report the wrong cause.
    let mut episode: Option<RecoveryEpisode> = None;
    // The client armed by one dead probe round. A second consecutive dead
    // round on the same connection severs it; an answer or a replacement
    // client disarms it.
    let mut pending_dead: Option<DaemonClient> = None;
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
            // The remote daemon's process is out of reach — a dropped
            // connection is all the episode can record.
            episode
                .get_or_insert_with(|| RecoveryEpisode::new(DaemonRecoveryCause::Disconnect, None));
            mark_outage(&inner);
            match DaemonClient::connect_before(
                &address,
                token.clone(),
                resume_from,
                Instant::now() + CONNECT_ATTEMPT_TIMEOUT,
            ) {
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
                    mark_connected(&inner, episode.take());
                }
                Err(error) => {
                    consecutive_failures += 1;
                    next_retry = Instant::now() + retry_delay(consecutive_failures);
                    eprintln!("could not reconnect to Goddard daemon: {error:#}");
                    note_recovery_failure(
                        &inner,
                        episode.as_mut().expect("an episode opened above"),
                        consecutive_failures,
                        false,
                    );
                }
            }
            continue;
        }
        let (down, client, endpoint) = {
            let mut target = inner.target.lock();
            match &mut *target {
                DaemonTarget::Local(process) => (
                    local_down(process),
                    process.client(),
                    Some((process.address.clone(), process.token.clone())),
                ),
                DaemonTarget::Restarting(client) => {
                    (Some(LocalDown::Respawning), client.clone(), None)
                }
                DaemonTarget::Remote {
                    client,
                    address,
                    token,
                } => (None, client.clone(), Some((address.clone(), token.clone()))),
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
                let _restart = inner.restart.lock();
                // Re-check under the restart lock: `reconfigure` may have
                // swapped in a fresh daemon while this thread waited.
                let still_down = match &mut *inner.target.lock() {
                    DaemonTarget::Local(process) => local_down(process),
                    DaemonTarget::Restarting(_) => Some(LocalDown::Respawning),
                    DaemonTarget::Remote { .. } => None,
                };
                if still_down.is_none() && !executable_changed {
                    mark_connected(&inner, episode.take());
                    continue;
                }
                mark_outage(&inner);
                // A downed daemon owns the cause even when a rebuild is also
                // pending: the process exit is what interrupted sessions. A
                // later observed exit updates the episode's exit detail — an
                // opening disconnect can still end in a real death, or a
                // replacement can crash-loop.
                let fresh = episode.is_none();
                match still_down {
                    Some(LocalDown::Exited(status)) => {
                        let exit = status.map_or(
                            DaemonExit {
                                code: None,
                                signal: None,
                            },
                            |status| DaemonExit::from_status(&status),
                        );
                        match &mut episode {
                            Some(open) => open.exit = Some(exit),
                            slot @ None => {
                                *slot = Some(RecoveryEpisode::new(
                                    DaemonRecoveryCause::UnexpectedExit,
                                    Some(exit),
                                ));
                            }
                        }
                    }
                    Some(LocalDown::Disconnected) => {
                        episode.get_or_insert_with(|| {
                            RecoveryEpisode::new(DaemonRecoveryCause::Disconnect, None)
                        });
                    }
                    Some(LocalDown::Respawning) | None => {
                        episode.get_or_insert_with(|| {
                            RecoveryEpisode::new(DaemonRecoveryCause::Rebuild, None)
                        });
                    }
                }
                // A daemon that dies within STABLE_UPTIME of its own launch
                // is crash-looping; an older one had a stable run and gets
                // an immediate replacement. `Respawning` can't be fresh —
                // an episode is always open by the time a swap started.
                if fresh && still_down.is_some() {
                    if healthy_since.elapsed() < STABLE_UPTIME {
                        consecutive_failures += 1;
                    }
                    if consecutive_failures > 0 {
                        next_retry = Instant::now() + retry_delay(consecutive_failures);
                        note_recovery_failure(
                            &inner,
                            episode.as_mut().expect("an episode opened above"),
                            consecutive_failures,
                            false,
                        );
                        continue;
                    }
                }
                // A daemon whose process is still alive only lost its
                // connection — reconnect in place; killing it would take
                // every provider runtime down for nothing. Connection
                // health alone never earns a replacement no matter how long
                // the outage runs: only a confirmed process exit (above)
                // or an explicit restart reaches `replace_local_daemon`.
                // The failure count survives a successful reconnect — it
                // clears only after a stable stretch — so a daemon that
                // flaps still degrades the status and the report, it just
                // is never killed for it.
                if matches!(still_down, Some(LocalDown::Disconnected)) {
                    match reconnect_local_daemon(&inner) {
                        Ok(()) => {
                            healthy_since = Instant::now();
                            mark_connected(&inner, episode.take());
                        }
                        Err(error) => {
                            consecutive_failures += 1;
                            next_retry = Instant::now() + retry_delay(consecutive_failures);
                            eprintln!("could not reconnect to the Goddard daemon: {error:#}");
                            note_recovery_failure(
                                &inner,
                                episode.as_mut().expect("an episode opened above"),
                                consecutive_failures,
                                true,
                            );
                        }
                    }
                    continue;
                }
                let Some(exposure) = inner.exposure.lock().clone() else {
                    return;
                };
                // The replacement kills the old process whether or not the
                // spawn succeeds — the episode's `replaced` reflects that
                // commitment from the attempt onward.
                if let Some(open) = episode.as_mut() {
                    open.replaced = true;
                }
                match replace_local_daemon(&inner, executable, &exposure) {
                    Ok(()) => {
                        healthy_since = Instant::now();
                        mark_connected(&inner, episode.take());
                        queue_settings_refresh(&inner);
                        if let Some(observed_stamp) = observed_stamp {
                            active_stamp = Some(observed_stamp);
                        }
                    }
                    Err(error) => {
                        consecutive_failures += 1;
                        next_retry = Instant::now() + retry_delay(consecutive_failures);
                        eprintln!("could not restart the Goddard daemon: {error:#}");
                        note_recovery_failure(
                            &inner,
                            episode.as_mut().expect("an episode opened above"),
                            consecutive_failures,
                            false,
                        );
                    }
                }
                continue;
            }
            episode = None;
            if consecutive_failures > 0 && healthy_since.elapsed() > STABLE_UPTIME {
                consecutive_failures = 0;
                set_status(&inner, DaemonStatus::Connected);
            }
        }
        // The socket being open only proves the socket is open. Probe the
        // request pipeline so a wedged daemon is declared dead and replaced
        // instead of hanging every request for the full request timeout. An
        // armed round re-probes on the next poll instead of the interval,
        // so escalation adds about one poll cycle to a genuine outage.
        if (pending_dead.is_some() || last_probe.elapsed() >= PROBE_INTERVAL)
            && !client.is_disconnected()
        {
            last_probe = Instant::now();
            if !client.probe(PROBE_TIMEOUT) {
                // A stalled pipeline on the app's busy socket says nothing
                // about the daemon — confirm on a fresh connection before
                // declaring it dead.
                let daemon_dead = endpoint.as_ref().is_none_or(|(address, token)| {
                    !probe_daemon_endpoint(address, token, PROBE_TIMEOUT)
                });
                if daemon_dead && dead_probe_escalates(&mut pending_dead, &client) {
                    client.force_disconnect();
                } else if !daemon_dead {
                    pending_dead = None;
                }
            } else {
                pending_dead = None;
            }
        }
    }
}

/// One dead probe round arms the next check; a second consecutive dead
/// round on the same connection severs it. Without the streak a daemon
/// that stalls for a few seconds — a checkpoint burst, a contested lock —
/// would flap every session it owns. A replacement client cannot inherit
/// an armed round: `same_connection` compares connection identity.
fn dead_probe_escalates(pending: &mut Option<DaemonClient>, client: &DaemonClient) -> bool {
    match pending.take() {
        Some(dead) if dead.same_connection(client) => true,
        _ => {
            *pending = Some(client.clone());
            false
        }
    }
}

/// A second opinion on daemon liveness through a brand-new connection —
/// the accept loop plus one request — run on a helper thread so a wedged
/// listener cannot stall the supervisor past `timeout`. The connect and
/// the probe share the deadline, so the helper always exits with it.
fn probe_daemon_endpoint(address: &str, token: &str, timeout: Duration) -> bool {
    let (done, done_rx) = mpsc::sync_channel(1);
    let _ = std::thread::Builder::new()
        .name("goddard-daemon-probe".into())
        .spawn({
            let address = address.to_owned();
            let token = token.to_owned();
            move || {
                let deadline = Instant::now() + timeout;
                let answered =
                    DaemonClient::connect_before(&address, token, Vec::new(), deadline)
                        .map(|client| client.probe(remaining_budget(deadline)))
                        .unwrap_or(false);
                let _ = done.send(answered);
            }
        });
    done_rx.recv_timeout(timeout).unwrap_or(false)
}

fn retry_delay(failures: u32) -> Duration {
    let shift = failures.saturating_sub(1).min(5);
    (RETRY_BASE_DELAY * 2_u32.pow(shift)).min(RETRY_MAX_DELAY)
}

/// The budget left before `deadline`, collapsed to a zero wait once it has
/// passed — the operation then fails immediately, which is the deadline's
/// job.
fn remaining_budget(deadline: Instant) -> Duration {
    deadline
        .checked_duration_since(Instant::now())
        .unwrap_or(Duration::ZERO)
}

fn set_status(inner: &SupervisorInner, status: DaemonStatus) {
    let mut current = inner.status.lock();
    if *current != status {
        *current = status;
        drop(current);
        inner
            .status_updates
            .lock()
            .retain(|subscriber| subscriber.send(status).is_ok());
    }
}

/// A retry attempt is starting: Connected degrades to Recovering, but an
/// already-announced Unreachable is never walked back by the next attempt
/// in the same outage.
fn mark_outage(inner: &SupervisorInner) {
    let mut status = inner.status.lock();
    if *status == DaemonStatus::Connected {
        *status = DaemonStatus::Recovering;
        drop(status);
        inner
            .status_updates
            .lock()
            .retain(|subscriber| subscriber.send(DaemonStatus::Recovering).is_ok());
    }
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
    episode: &RecoveryEpisode,
    outcome: DaemonRecoveryOutcome,
) {
    inner.recovery_reports.lock().retain(|subscriber| {
        subscriber
            .send(DaemonRecovery {
                cause: episode.cause,
                outcome,
                exit: episode.exit,
                replaced: episode.replaced,
            })
            .is_ok()
    });
}

/// Recovery succeeded — report it only when the daemon was actually
/// recovering, so steady-state bookkeeping never reads as an outage. The
/// episode is consumed: one recovery transition per continuous outage.
fn mark_connected(inner: &SupervisorInner, episode: Option<RecoveryEpisode>) {
    let recovered = *inner.status.lock() != DaemonStatus::Connected;
    set_status(inner, DaemonStatus::Connected);
    if recovered && let Some(episode) = episode {
        report_recovery(inner, &episode, DaemonRecoveryOutcome::Recovered);
    }
}

/// A recovery attempt failed. The status ratchets Connected → Recovering →
/// the episode's outage status — `Degraded` for a managed daemon still
/// alive under a dead connection, `Unreachable` for a dead, respawning, or
/// remote daemon — and the crossing is announced once per episode; slow
/// retries keep calling this and must not repeat the same report.
fn note_recovery_failure(
    inner: &SupervisorInner,
    episode: &mut RecoveryEpisode,
    failures: u32,
    process_alive: bool,
) {
    let past_threshold = failures >= UNREACHABLE_AFTER_FAILURES;
    let current = *inner.status.lock();
    // Retries never regress an announced outage back to Recovering — the
    // episode ends only on a working connection.
    let degraded = match (past_threshold, current) {
        (true, _) if process_alive => DaemonStatus::Degraded,
        (true, _) => DaemonStatus::Unreachable,
        (_, DaemonStatus::Unreachable | DaemonStatus::Degraded) => current,
        _ => DaemonStatus::Recovering,
    };
    set_status(inner, degraded);
    if past_threshold && !episode.unreachable_announced {
        episode.unreachable_announced = true;
        report_recovery(inner, episode, DaemonRecoveryOutcome::Unreachable);
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
    let client = DaemonClient::connect_before(
        &address,
        token,
        resume_from,
        Instant::now() + CONNECT_ATTEMPT_TIMEOUT,
    )?;
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
    // One deadline covers the replacement's whole startup — spawn, ready
    // line, and control-socket connect share it.
    let deadline = Instant::now() + START_TIMEOUT;
    let replacement = DaemonProcess::spawn_configured(executable, exposure.clone(), deadline)?;
    if let Err(error) = apply_daemon_exposure(&replacement.client(), exposure, deadline) {
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
/// listener, so there is nothing to send. The request shares the startup
/// `deadline` so a slow daemon fails bounded.
fn apply_daemon_exposure(
    client: &DaemonClient,
    exposure: &DaemonExposureSettings,
    deadline: Instant,
) -> anyhow::Result<()> {
    if !exposure.enabled {
        return Ok(());
    }
    set_daemon_exposure(client, exposure.wire(), remaining_budget(deadline))
}

/// Send `setDaemonExposure` and verify the daemon understood it.
fn set_daemon_exposure(
    client: &DaemonClient,
    exposure: Option<waku_protocol::DaemonExposure>,
    timeout: Duration,
) -> anyhow::Result<()> {
    match client.request_with_timeout(
        Uuid::nil(),
        Uuid::nil(),
        Command::SetDaemonExposure { exposure },
        Some(timeout),
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

fn read_settings(client: &DaemonClient, timeout: Duration) -> anyhow::Result<DaemonSettings> {
    match client.request_with_timeout(
        Uuid::nil(),
        Uuid::nil(),
        Command::GetSettings,
        Some(timeout),
    )? {
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

    /// Answers the `Hello` handshake on every accepted connection so probe
    /// clients can be fabricated without a real daemon.
    fn hello_endpoint() -> String {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap().to_string();
        std::thread::spawn(move || {
            loop {
                let Ok((stream, _)) = listener.accept() else {
                    return;
                };
                std::thread::spawn(move || {
                    let Ok(mut socket) = tungstenite::accept(stream) else {
                        return;
                    };
                    while let Ok(message) = socket.read() {
                        let tungstenite::Message::Text(text) = message else {
                            continue;
                        };
                        let Ok(waku_protocol::ClientMessage::Hello { .. }) =
                            serde_json::from_str::<waku_protocol::ClientMessage>(text.as_ref())
                        else {
                            continue;
                        };
                        let reply = serde_json::to_string(&waku_protocol::ServerMessage::Hello {
                            protocol_version: PROTOCOL_VERSION,
                            daemon_version: "test".into(),
                            daemon_commit: None,
                            agent_cli_available: false,
                        })
                        .unwrap();
                        if socket
                            .send(tungstenite::Message::Text(reply.into()))
                            .is_err()
                        {
                            return;
                        }
                    }
                });
            }
        });
        address
    }

    /// Two consecutive dead rounds on the same connection must sever it; a
    /// single round — or a round on a different connection — only arms the
    /// next check.
    #[test]
    fn dead_probe_escalates_only_on_the_same_connection_twice() {
        let address = hello_endpoint();
        let client = DaemonClient::connect(&address, "token".into()).unwrap();
        let replacement = DaemonClient::connect(&address, "token".into()).unwrap();
        let mut pending = None;

        assert!(!dead_probe_escalates(&mut pending, &client));
        assert!(dead_probe_escalates(&mut pending, &client));

        assert!(!dead_probe_escalates(&mut pending, &replacement));
        assert!(dead_probe_escalates(&mut pending, &replacement));
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

    /// A listener that accepts every connection and holds it open, silent —
    /// the client's own deadline is what ends each attempt.
    fn black_hole_endpoint() -> String {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap().to_string();
        std::thread::spawn(move || {
            while let Ok((stream, _)) = listener.accept() {
                std::thread::spawn(move || {
                    let _stream = stream;
                    loop {
                        std::thread::sleep(Duration::from_secs(60));
                    }
                });
            }
        });
        address
    }

    /// A supervisor with nothing behind it but a connected remote client —
    /// enough for the reporting helpers to fan out to subscribers.
    fn test_supervisor(address: &str) -> DaemonSupervisor {
        let client = DaemonClient::connect(address, "token".into()).unwrap();
        DaemonSupervisor::from_target(
            DaemonTarget::Remote {
                client,
                address: address.to_owned(),
                token: "token".into(),
            },
            None,
            true,
            None,
            DaemonSettings::default(),
        )
        .unwrap()
    }

    /// TCP connect succeeds against a silent listener, so only the deadline
    /// bounds the handshake — the attempt must fail at the deadline, not
    /// hang on the OS's own connect timeout.
    #[test]
    fn a_stalled_connect_fails_at_its_deadline() {
        let address = black_hole_endpoint();
        let deadline = Duration::from_millis(400);
        let started = Instant::now();
        let result = DaemonClient::connect_before(
            &address,
            "token".into(),
            Vec::new(),
            Instant::now() + deadline,
        );
        assert!(result.is_err());
        assert!(
            started.elapsed() < deadline * 4,
            "connect outlived its deadline"
        );
    }

    /// The second-opinion probe must fail inside its timeout against a
    /// daemon that accepts but never answers — and its helper thread must
    /// exit with the attempt rather than linger on a dead socket.
    #[test]
    fn a_stalled_endpoint_probe_fails_inside_its_timeout() {
        let address = black_hole_endpoint();
        let started = Instant::now();
        assert!(!probe_daemon_endpoint(
            &address,
            "token".into(),
            Duration::from_millis(500)
        ));
        assert!(
            started.elapsed() < Duration::from_secs(4),
            "the probe helper outlived its timeout"
        );
    }

    /// Dropping the last client handle must end the socket thread — a
    /// wedged-daemon probe's abandoned client used to spin on a 25ms poll
    /// forever, holding the daemon's connection open.
    #[test]
    fn a_dropped_client_lets_its_reader_close_the_socket() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap().to_string();
        let (closed, closed_rx) = mpsc::sync_channel(1);
        std::thread::spawn(move || {
            let Ok((stream, _)) = listener.accept() else {
                return;
            };
            let Ok(mut socket) = tungstenite::accept(stream) else {
                return;
            };
            loop {
                match socket.read() {
                    Ok(tungstenite::Message::Text(text)) => {
                        if serde_json::from_str::<waku_protocol::ClientMessage>(text.as_ref())
                            .is_ok_and(|message| {
                                matches!(message, waku_protocol::ClientMessage::Hello { .. })
                            })
                        {
                            let reply = serde_json::to_string(
                                &waku_protocol::ServerMessage::Hello {
                                    protocol_version: PROTOCOL_VERSION,
                                    daemon_version: "test".into(),
                                    daemon_commit: None,
                                    agent_cli_available: false,
                                },
                            )
                            .unwrap();
                            if socket
                                .send(tungstenite::Message::Text(reply.into()))
                                .is_err()
                            {
                                return;
                            }
                        }
                    }
                    // The socket going away is the assertion signal.
                    Ok(tungstenite::Message::Close(_)) | Err(_) => {
                        let _ = closed.send(());
                        return;
                    }
                    Ok(_) => {}
                }
            }
        });
        let client = DaemonClient::connect(&address, "token".into()).unwrap();
        drop(client);
        closed_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("a dropped client's socket stayed open");
    }

    /// One continuous outage on a live-but-silent daemon crosses the
    /// threshold into `Degraded` exactly once; retries inside the episode
    /// neither re-announce it nor walk the status back to Recovering.
    #[test]
    fn a_live_daemon_outage_degrades_once_then_recovers_once() {
        let supervisor = test_supervisor(&hello_endpoint());
        let reports = supervisor.subscribe_recovery();
        let mut episode = RecoveryEpisode::new(DaemonRecoveryCause::Disconnect, None);
        for failures in 1..8 {
            note_recovery_failure(&supervisor.inner, &mut episode, failures, true);
        }
        let seen: Vec<DaemonRecovery> = reports.try_iter().collect();
        assert_eq!(seen.len(), 1, "the episode re-announced the crossing");
        assert_eq!(seen[0].outcome, DaemonRecoveryOutcome::Unreachable);
        assert!(matches!(seen[0].cause, DaemonRecoveryCause::Disconnect));
        assert!(!seen[0].replaced);
        assert_eq!(supervisor.status(), DaemonStatus::Degraded);

        // A retry inside the same outage never regresses the status.
        note_recovery_failure(&supervisor.inner, &mut episode, 1, true);
        assert_eq!(supervisor.status(), DaemonStatus::Degraded);
        assert!(reports.try_iter().next().is_none());

        // Connectivity back: one recovery transition carries the episode.
        mark_connected(&supervisor.inner, Some(episode));
        let seen: Vec<DaemonRecovery> = reports.try_iter().collect();
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].outcome, DaemonRecoveryOutcome::Recovered);
        assert!(!seen[0].replaced, "an in-place reconnect is not a replacement");
        assert_eq!(supervisor.status(), DaemonStatus::Connected);
    }

    /// An episode for a daemon that actually died — or whose process the
    /// supervisor cannot see — crosses to `Unreachable` instead.
    #[test]
    fn a_dead_or_unmanaged_daemon_outage_is_unreachable_not_degraded() {
        let supervisor = test_supervisor(&hello_endpoint());
        let reports = supervisor.subscribe_recovery();
        let mut episode = RecoveryEpisode::new(
            DaemonRecoveryCause::UnexpectedExit,
            Some(DaemonExit {
                code: None,
                signal: Some(9),
            }),
        );
        for failures in 1..8 {
            note_recovery_failure(&supervisor.inner, &mut episode, failures, false);
        }
        let seen: Vec<DaemonRecovery> = reports.try_iter().collect();
        assert_eq!(seen.len(), 1);
        assert_eq!(supervisor.status(), DaemonStatus::Unreachable);
        assert_eq!(seen[0].exit.unwrap().signal, Some(9));
    }

    /// A process still running with a dead connection is `Disconnected`,
    /// not `Exited` — the distinction that keeps reconnect-in-place ahead
    /// of a kill.
    #[cfg(unix)]
    #[test]
    fn a_live_process_with_a_dead_connection_is_disconnected_not_exited() {
        let address = hello_endpoint();
        let client = DaemonClient::connect(&address, "token".into()).unwrap();
        client.force_disconnect();
        let child = ProcessCommand::new("sleep").arg("60").spawn().unwrap();
        let mut process = DaemonProcess {
            client,
            child,
            address,
            token: "token".into(),
        };
        assert!(matches!(
            local_down(&mut process),
            Some(LocalDown::Disconnected)
        ));
    }
}
