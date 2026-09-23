//! The Diagnostics page's backing data: one normalized feed over the app's
//! reported-error journal and the daemon's forensic logs — the on-disk
//! signals `docs/daemon-diagnostics.md` catalogs, read for the interface.
//!
//! Writing happens in `record_app_error`, reached by every error toast; the
//! page itself only reads, merging the three files at load time so no
//! existing schema changes.

use std::io::Write as _;
use std::path::{Path, PathBuf};

/// One feed row — the normalized shape every source maps into. `detail` is
/// the raw record pretty-printed: what a row's Copy puts on the clipboard
/// for a bug report.
#[derive(Clone, Debug)]
pub(crate) struct DiagnosticEntry {
    /// Unix seconds.
    pub(crate) at: u64,
    pub(crate) source: DiagnosticSource,
    /// The one-line summary a row shows.
    pub(crate) summary: String,
    pub(crate) detail: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DiagnosticSource {
    /// An error or command-failure toast the app showed.
    AppError,
    /// A supervisor restart or reconnect episode.
    DaemonRecovery,
    /// A daemon panic.
    DaemonPanic,
}

/// The most entries a visit reads back — the feed shows recent history,
/// not the files' full tails.
pub(crate) const MAX_ENTRIES: usize = 200;

/// ~256 KB at a few hundred bytes a line is years of reported errors — the
/// bound the recovery journal already uses.
const ERRORS_LOG_CAP: u64 = 256 * 1024;

fn errors_log_path() -> Option<PathBuf> {
    Some(dirs::home_dir()?.join(".goddard").join("errors.jsonl"))
}

/// The folder the Diagnostics page's reveal button opens — home of
/// `errors.jsonl` and `daemon-recovery.jsonl`. `daemon-panics.jsonl` lives
/// in the daemon's own data directory instead.
pub(crate) fn logs_directory() -> Option<PathBuf> {
    Some(dirs::home_dir()?.join(".goddard"))
}

/// The directory the local daemon writes `daemon-panics.jsonl` into — the
/// parent of its `app.db`, so `GODDARD_DATA_DIR` debug instances read their
/// own daemon's files.
fn daemon_data_dir() -> Option<PathBuf> {
    if cfg!(debug_assertions)
        && let Some(dir) = std::env::var_os("GODDARD_DATA_DIR").filter(|dir| !dir.is_empty())
    {
        return Some(PathBuf::from(dir));
    }
    Some(dirs::data_local_dir()?.join(waku_protocol::identity::DATA_DIRECTORY_NAME))
}

/// Append one reported error to `~/.goddard/errors.jsonl`. Callers run it
/// on a background executor — every error toast funnels here, so the write
/// must never reach a frame. The file self-caps by keeping the newest half
/// at a line boundary, the scheme `daemon-stats.jsonl` uses.
pub(crate) fn record_app_error(kind: &'static str, message: &str) {
    let Some(path) = errors_log_path() else {
        return;
    };
    let record = serde_json::json!({
        "at": crate::model::unix_time(),
        "kind": kind,
        "message": message,
    });
    let _ = append_capped_line(&path, &record.to_string());
}

/// Read every source into one newest-first feed capped at `MAX_ENTRIES`.
/// Lenient throughout: an unreadable file or a torn tail line drops that
/// one record, never the feed.
pub(crate) fn load_entries() -> Vec<DiagnosticEntry> {
    let mut entries = Vec::new();
    if let Some(path) = errors_log_path() {
        read_jsonl(&path, app_error_entry, &mut entries);
    }
    if let Some(home) = dirs::home_dir() {
        read_jsonl(
            &home.join(".goddard").join("daemon-recovery.jsonl"),
            recovery_entry,
            &mut entries,
        );
    }
    if let Some(dir) = daemon_data_dir() {
        read_jsonl(&dir.join("daemon-panics.jsonl"), panic_entry, &mut entries);
    }
    entries.sort_by(|a, b| b.at.cmp(&a.at));
    entries.truncate(MAX_ENTRIES);
    entries
}

fn read_jsonl(
    path: &Path,
    map: impl Fn(&serde_json::Value, &str) -> Option<DiagnosticEntry>,
    entries: &mut Vec<DiagnosticEntry>,
) {
    let Ok(text) = std::fs::read_to_string(path) else {
        return;
    };
    for line in text.lines() {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if let Some(entry) = map(&value, line) {
            entries.push(entry);
        }
    }
}

fn app_error_entry(value: &serde_json::Value, raw: &str) -> Option<DiagnosticEntry> {
    Some(DiagnosticEntry {
        at: value["at"].as_u64()?,
        source: DiagnosticSource::AppError,
        summary: value["message"].as_str()?.to_owned(),
        detail: pretty(raw),
    })
}

fn recovery_entry(value: &serde_json::Value, raw: &str) -> Option<DiagnosticEntry> {
    let cause = value["cause"].as_str().unwrap_or("unknown");
    let outcome = value["outcome"].as_str().unwrap_or("unknown");
    let mut summary = tr!(
        "diagnostics.recovery_summary",
        cause = cause,
        outcome = outcome
    );
    let mut extras: Vec<String> = Vec::new();
    if let Some(signal) = value["exitSignal"].as_str().filter(|s| !s.is_empty()) {
        extras.push(signal.to_owned());
    } else if let Some(code) = value["exitCode"].as_i64() {
        extras.push(format!("exit {code}"));
    }
    match value["sessionsResumed"].as_u64().unwrap_or_default() {
        0 => {}
        1 => extras.push(tr!("diagnostics.sessions_resumed_one")),
        resumed => extras.push(tr!("diagnostics.sessions_resumed_many", count = resumed)),
    }
    // A remote daemon's episode names its host; the local one goes
    // unmarked since nearly every record is local.
    if let Some(daemon) = value["daemon"].as_str().filter(|name| *name != "local") {
        extras.push(daemon.to_owned());
    }
    if !extras.is_empty() {
        summary = format!("{summary} · {}", extras.join(" · "));
    }
    Some(DiagnosticEntry {
        at: value["at"].as_u64()?,
        source: DiagnosticSource::DaemonRecovery,
        summary,
        detail: pretty(raw),
    })
}

fn panic_entry(value: &serde_json::Value, raw: &str) -> Option<DiagnosticEntry> {
    let message = value["message"].as_str().unwrap_or_default();
    let location = value["location"].as_str().unwrap_or_default();
    let summary = match (message.is_empty(), location.is_empty()) {
        (false, false) => format!("{message} · {location}"),
        (false, true) => message.to_owned(),
        (true, false) => location.to_owned(),
        (true, true) => tr!("diagnostics.panic_unnamed"),
    };
    Some(DiagnosticEntry {
        at: value["at"].as_u64()?,
        source: DiagnosticSource::DaemonPanic,
        summary,
        detail: pretty(raw),
    })
}

/// The raw line re-wrapped for a clipboard paste — parseable stays JSON,
/// just readable.
fn pretty(raw: &str) -> String {
    serde_json::from_str::<serde_json::Value>(raw)
        .ok()
        .and_then(|value| serde_json::to_string_pretty(&value).ok())
        .unwrap_or_else(|| raw.to_owned())
}

/// Append a line, then cap the file by keeping the newest half cut at a
/// line boundary — the same scheme `daemon-stats.jsonl` and the recovery
/// journal use.
fn append_capped_line(path: &Path, line: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    writeln!(file, "{line}")?;
    drop(file);
    if std::fs::metadata(path).map(|meta| meta.len()).unwrap_or(0) <= ERRORS_LOG_CAP {
        return Ok(());
    }
    let bytes = std::fs::read(path)?;
    let halfway = bytes.len() / 2;
    let start = bytes[halfway..]
        .iter()
        .position(|byte| *byte == b'\n')
        .map(|offset| halfway + offset + 1)
        .unwrap_or(bytes.len());
    std::fs::write(path, &bytes[start..])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_journal_line_reads_back_as_an_app_error() {
        let path =
            std::env::temp_dir().join(format!("goddard-errors-{}.jsonl", uuid::Uuid::new_v4()));
        let line = serde_json::json!({
            "at": 1_700_000_000_u64,
            "kind": "alert",
            "message": "Couldn't create worktree",
        })
        .to_string();
        std::fs::write(&path, format!("{line}\n")).unwrap();

        let mut entries = Vec::new();
        read_jsonl(&path, app_error_entry, &mut entries);
        let _ = std::fs::remove_file(&path);

        assert_eq!(entries.len(), 1);
        let entry = &entries[0];
        assert_eq!(entry.at, 1_700_000_000);
        assert_eq!(entry.source, DiagnosticSource::AppError);
        assert_eq!(entry.summary, "Couldn't create worktree");
        assert!(entry.detail.contains("Couldn't create worktree"));
    }

    #[test]
    fn a_torn_tail_line_does_not_hide_the_good_records() {
        let path =
            std::env::temp_dir().join(format!("goddard-errors-{}.jsonl", uuid::Uuid::new_v4()));
        std::fs::write(
            &path,
            concat!(
                r#"{"at":1,"kind":"alert","message":"first"}"#,
                "\n",
                "{torn tail\n",
            ),
        )
        .unwrap();

        let mut entries = Vec::new();
        read_jsonl(&path, app_error_entry, &mut entries);
        let _ = std::fs::remove_file(&path);

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].summary, "first");
    }

    #[test]
    fn recovery_records_compose_a_summary_from_their_fields() {
        let value = serde_json::json!({
            "at": 2_u64,
            "daemon": "local",
            "cause": "unexpected_exit",
            "outcome": "recovered",
            "sessionsResumed": 3,
            "exitSignal": "SIGKILL",
        });
        let entry = recovery_entry(&value, "{}").unwrap();
        assert_eq!(entry.source, DiagnosticSource::DaemonRecovery);
        assert!(entry.summary.contains("unexpected_exit"));
        assert!(entry.summary.contains("recovered"));
        assert!(entry.summary.contains("SIGKILL"));
        assert!(entry.summary.contains("3"));
    }
}
