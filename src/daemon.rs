//! Desktop ownership of the Goddard daemon process.

use std::path::PathBuf;

use anyhow::{Context as _, anyhow, bail};

pub fn start_process() -> anyhow::Result<waku_client::DaemonSupervisor> {
    let address = std::env::var(waku_client::DAEMON_ADDRESS_ENV)
        .ok()
        .filter(|value| !value.trim().is_empty());
    let token = std::env::var(waku_client::DAEMON_TOKEN_ENV)
        .ok()
        .filter(|value| !value.is_empty());
    match (address, token) {
        (Some(address), Some(token)) => {
            return waku_client::DaemonSupervisor::connect(address.trim(), token);
        }
        (Some(_), None) => bail!(
            "{} is set but {} is missing; set both or neither",
            waku_client::DAEMON_ADDRESS_ENV,
            waku_client::DAEMON_TOKEN_ENV
        ),
        (None, Some(_)) => bail!(
            "{} is set but {} is missing; set both or neither",
            waku_client::DAEMON_TOKEN_ENV,
            waku_client::DAEMON_ADDRESS_ENV
        ),
        (None, None) => {}
    }
    let app_settings = waku_client::persistence::load_or_create_app_settings()
        .context("could not load desktop daemon settings")?;
    waku_client::DaemonSupervisor::spawn_configured(
        &daemon_executable_path()?,
        cfg!(debug_assertions),
        app_settings.daemon_exposure,
    )
}

/// Resolve the local host name once during app construction. Settings can
/// then show a useful LAN URL without touching the OS from a render frame.
pub fn local_hostname() -> Option<String> {
    #[cfg(unix)]
    {
        let mut buffer = [0_u8; 256];
        let result = unsafe { libc::gethostname(buffer.as_mut_ptr().cast(), buffer.len()) };
        if result == 0 {
            let length = buffer
                .iter()
                .position(|byte| *byte == 0)
                .unwrap_or(buffer.len());
            let hostname = String::from_utf8_lossy(&buffer[..length]).trim().to_owned();
            if !hostname.is_empty() {
                return Some(hostname);
            }
        }
    }
    // `COMPUTERNAME` is the Windows equivalent and is always set; `HOSTNAME`
    // covers the shells that export it.
    ["COMPUTERNAME", "HOSTNAME"]
        .into_iter()
        .filter_map(|name| std::env::var(name).ok())
        .map(|hostname| hostname.trim().to_owned())
        .find(|hostname| !hostname.is_empty())
}

/// The LAN IPv4 a nearby device can dial directly — the address the daemon
/// settings QR encodes, since phones cannot resolve this machine's mDNS
/// hostname. `connect` on an unbound UDP socket only resolves a route, so
/// nothing is sent; the kernel reports the default egress interface's
/// source address. Resolved once at app construction.
pub fn local_ipv4() -> Option<String> {
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("192.0.0.1:80").ok()?;
    match socket.local_addr().ok()?.ip() {
        std::net::IpAddr::V4(ip) if !ip.is_loopback() => Some(ip.to_string()),
        _ => None,
    }
}

/// One JSON line per supervisor recovery episode, appended to
/// `~/.goddard/daemon-recovery.jsonl` — the on-machine counterpart of the
/// `daemon.recovery` analytics event, so a restart's cause and exit detail
/// are diagnosable without leaving the box. Append-only and self-capping
/// like the daemon's own `daemon-stats.jsonl`.
pub(crate) fn log_daemon_recovery(record: &serde_json::Value) {
    let Some(home) = dirs::home_dir() else {
        return;
    };
    let path = home.join(".goddard").join("daemon-recovery.jsonl");
    if append_json_line(&path, record).is_err() {
        return;
    }
    if std::fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0) <= RECOVERY_LOG_CAP {
        return;
    }
    // Keep the newest half, cut at a line boundary — same scheme the
    // daemon's stats sampler uses.
    if let Ok(bytes) = std::fs::read(&path) {
        let halfway = bytes.len() / 2;
        let start = bytes[halfway..]
            .iter()
            .position(|byte| *byte == b'\n')
            .map(|offset| halfway + offset + 1)
            .unwrap_or(bytes.len());
        let _ = std::fs::write(&path, &bytes[start..]);
    }
}

/// ~256 KB at a few hundred bytes a line is years of recovery episodes.
const RECOVERY_LOG_CAP: u64 = 256 * 1024;

fn append_json_line(path: &std::path::Path, record: &serde_json::Value) -> std::io::Result<()> {
    use std::io::Write as _;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    writeln!(file, "{record}")?;
    Ok(())
}

/// One launch-time pass over `~/Library/Logs/DiagnosticReports` for crash
/// reports the OS wrote for a daemon process — one `daemon.crash` event
/// per report not yet seen, deduplicated by a file-mtime watermark in
/// `~/.goddard/daemon-crashes.json`. Runs on a background executor; the
/// first run only establishes the watermark because historical reports
/// predate the vocabulary.
pub fn spawn_crash_report_scan(
    analytics: crate::analytics::Analytics,
    executor: &gpui::BackgroundExecutor,
) {
    executor
        .spawn(async move { scan_new_crash_reports(&analytics) })
        .detach();
}

#[cfg(target_os = "macos")]
fn scan_new_crash_reports(analytics: &crate::analytics::Analytics) {
    let Some(home) = dirs::home_dir() else {
        return;
    };
    let watermark_path = home.join(".goddard").join("daemon-crashes.json");
    let last_seen = std::fs::read_to_string(&watermark_path)
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .and_then(|value| value["lastSeenAt"].as_u64());
    let Ok(entries) = std::fs::read_dir(home.join("Library/Logs/DiagnosticReports")) else {
        return;
    };
    let mut newest = last_seen.unwrap_or(0);
    let mut reports = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        let is_daemon_report = name.ends_with(".ips")
            && (name.starts_with("goddard-daemon-") || name.starts_with("goddard-debug-daemon-"));
        if !is_daemon_report {
            continue;
        }
        let Some(at) = entry
            .metadata()
            .and_then(|metadata| metadata.modified())
            .ok()
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|duration| duration.as_secs())
        else {
            continue;
        };
        newest = newest.max(at);
        if last_seen.is_some_and(|seen| at > seen)
            && let Ok(contents) = std::fs::read_to_string(entry.path())
            && let Some(fields) = report_fields(&contents)
        {
            reports.push(fields);
        }
    }
    for (termination, signal, uptime_secs) in reports {
        analytics.track(crate::analytics::Event::DaemonCrash {
            termination,
            signal,
            uptime_secs,
        });
    }
    let _ = std::fs::create_dir_all(watermark_path.parent().unwrap_or(&home));
    let _ = std::fs::write(
        &watermark_path,
        serde_json::json!({ "lastSeenAt": newest }).to_string(),
    );
}

#[cfg(not(target_os = "macos"))]
fn scan_new_crash_reports(_analytics: &crate::analytics::Analytics) {}

/// Pull the coarse fields out of an `.ips` crash report — the first line
/// is a JSON metadata header, the rest is the report body. Returns the
/// termination namespace bucket, the crashing signal name, and the
/// process uptime in seconds.
#[cfg(target_os = "macos")]
fn report_fields(contents: &str) -> Option<(&'static str, Option<String>, Option<u64>)> {
    let (_, body) = contents.split_once('\n')?;
    let body: serde_json::Value = serde_json::from_str(body).ok()?;
    let namespace = body
        .pointer("/termination/namespace")
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    Some((
        termination_bucket(namespace),
        body.pointer("/exception/signal")
            .and_then(|value| value.as_str())
            .map(str::to_owned),
        body.pointer("/uptime").and_then(|value| value.as_u64()),
    ))
}

/// The `termination.namespace` values macOS writes — `EXC_RESOURCE` is how
/// jetsam and CPU/wakeup limits present, `SIGNAL` covers crashes and
/// kills.
#[cfg(target_os = "macos")]
fn termination_bucket(namespace: &str) -> &'static str {
    match namespace {
        "SIGNAL" => "signal",
        "EXC_RESOURCE" => "exc_resource",
        "EXC_GUARD" => "exc_guard",
        "COREDUMP" => "coredump",
        "CODESIGNING" => "codesigning",
        "WATCHDOG" => "watchdog",
        _ => "other",
    }
}

pub(crate) fn daemon_executable_path() -> anyhow::Result<PathBuf> {
    if let Some(path) = std::env::var_os("GODDARD_DAEMON_PATH").filter(|path| !path.is_empty()) {
        return Ok(path.into());
    }
    let current = std::env::current_exe().context("could not locate the Goddard executable")?;
    daemon_executable_beside(&current)
}

fn daemon_executable_beside(current: &std::path::Path) -> anyhow::Result<PathBuf> {
    let executable = format!("goddard-daemon{}", std::env::consts::EXE_SUFFIX);

    let directory = current
        .parent()
        .ok_or_else(|| anyhow!("Goddard executable has no parent directory"))?;
    // Packaged debug and release apps own their daemon sibling. Never let a
    // debug-named ancestor redirect a bundle to another runtime.
    #[cfg(all(target_os = "macos", debug_assertions))]
    {
        let sibling = directory.join("goddard-debug-daemon");
        if sibling.is_file() {
            return Ok(sibling);
        }
    }
    let sibling = directory.join(&executable);
    if sibling.is_file() {
        return Ok(sibling);
    }
    // Only an unbundled cargo run uses the ancestor debug directory fallback.
    #[cfg(debug_assertions)]
    if !current
        .ancestors()
        .any(|path| path.extension().is_some_and(|ext| ext == "app"))
    {
        if let Some(debug_directory) = current
            .ancestors()
            .find(|candidate| candidate.file_name().is_some_and(|name| name == "debug"))
        {
            let external = debug_directory.join(&executable);
            if external.is_file() {
                return Ok(external);
            }
        }
    }
    #[cfg(debug_assertions)]
    bail!(
        "Goddard daemon was not found in Cargo's debug directory or next to the app executable: {}",
        sibling.display(),
    );
    #[cfg(not(debug_assertions))]
    bail!(
        "Goddard daemon is missing next to the app executable: {}",
        sibling.display(),
    )
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    #[cfg(all(target_os = "macos", debug_assertions))]
    #[test]
    fn bundled_daemon_wins_over_a_debug_ancestor() {
        let root = std::env::temp_dir().join(format!("daemon-lookup-{}", uuid::Uuid::new_v4()));
        let debug = root.join("debug");
        let macos = debug.join("lane/Goddard Debug.app/Contents/MacOS");
        std::fs::create_dir_all(&macos).unwrap();
        std::fs::write(debug.join("goddard-daemon"), b"foreign").unwrap();
        let sibling = macos.join("goddard-debug-daemon");
        std::fs::write(&sibling, b"bundled").unwrap();
        assert_eq!(
            super::daemon_executable_beside(&macos.join("Goddard Debug")).unwrap(),
            sibling
        );
        std::fs::remove_file(sibling).unwrap();
        // A broken bundle must fail rather than adopting the foreign runtime.
        assert!(super::daemon_executable_beside(&macos.join("Goddard Debug")).is_err());
        assert_eq!(
            super::daemon_executable_beside(&debug.join("goddard")).unwrap(),
            debug.join("goddard-daemon")
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn crash_report_fields_pull_termination_signal_and_uptime() {
        let report = concat!(
            r#"{"app_name":"goddard-daemon","timestamp":"2026-09-22 12:00:00.00 -0700"}"#,
            "\n",
            r#"{"procName":"goddard-daemon","uptime":3600,"termination":{"flags":0,"code":9,"namespace":"EXC_RESOURCE","indicator":"Killed: 9"},"exception":{"type":"EXC_RESOURCE","signal":"SIGKILL"},"faultingThread":0}"#
        );
        let (termination, signal, uptime) = report_fields(report).unwrap();
        assert_eq!(termination, "exc_resource");
        assert_eq!(signal.as_deref(), Some("SIGKILL"));
        assert_eq!(uptime, Some(3600));
    }

    #[test]
    fn crash_report_fields_tolerates_a_partial_body() {
        let report = "{\"app_name\":\"goddard-daemon\"}\n{\"uptime\":12}";
        let (termination, signal, uptime) = report_fields(report).unwrap();
        assert_eq!(termination, "other");
        assert_eq!(signal, None);
        assert_eq!(uptime, Some(12));
    }
}
