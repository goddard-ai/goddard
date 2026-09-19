//! Antigravity CLI (`agy`) session state, read off the CLI's own store.
//!
//! Goddard never parses agy's conversation transcripts — its sessions are
//! the CLI's own TUI. What the sidebar needs instead is liveness: whether a
//! conversation is still doing work, what it is titled, and which
//! conversation id a spawned terminal opened. All three live in
//! `~/.gemini/antigravity-cli`: `conversation_summaries.db` carries per
//! conversation status, `cache/last_conversations.json` maps a workspace
//! path to the conversation agy most recently opened there.
//!
//! Everything here is pure filesystem work for a background executor; none
//! of it may run on a frame.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// The CLI's data root under the user's home directory.
pub fn app_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|home| home.join(".gemini/antigravity-cli"))
}

fn summaries_db_path() -> Option<PathBuf> {
    app_dir().map(|dir| dir.join("conversation_summaries.db"))
}

fn last_conversations_path() -> Option<PathBuf> {
    app_dir().map(|dir| dir.join("cache/last_conversations.json"))
}

/// The summaries row can be timestamped a second before the spawn marker it
/// belongs to — the CLI stamps it on its own clock at TUI open while the app
/// records `seen_at` just before exec. A minute of slack still rejects any
/// genuinely stale row, which is always minutes or hours older.
const SEEN_AT_SLACK_SECS: u64 = 60;

/// What a spawned `agy` process opened in a workspace, from the CLI's own
/// "last conversation per workspace" map. `seen_at` bounds the answer: a
/// stale mapping left by an earlier, unrelated `agy` run in the same
/// directory reports a conversation whose summary row predates this spawn
/// and is rejected.
pub fn conversation_id_for_cwd(cwd: &Path, seen_at: u64) -> Option<String> {
    let path = last_conversations_path()?;
    let raw = std::fs::read_to_string(path).ok()?;
    let map: HashMap<String, String> = serde_json::from_str(&raw).ok()?;
    // The CLI keys the map by the cwd it was handed, which may be the
    // canonicalized path rather than the spelling the session stored
    // (`/tmp` vs `/private/tmp` on macOS). Both keys can exist with
    // different conversations, so check each: a stale entry must not
    // shadow a fresh one under the other spelling.
    let mut keys = vec![cwd.to_string_lossy().to_string()];
    if let Ok(canonical) = std::fs::canonicalize(cwd) {
        let key = canonical.to_string_lossy().to_string();
        if key != keys[0] {
            keys.push(key);
        }
    }
    for key in keys {
        let Some(id) = map.get(&key) else {
            continue;
        };
        // A conversation still missing from the summaries db is new enough
        // by definition; one present but unmodified since `seen_at` is stale.
        match conversation_summary(id) {
            Some(summary) if summary.last_modified_unix + SEEN_AT_SLACK_SECS >= seen_at => {
                return Some(id.clone());
            }
            None => return Some(id.clone()),
            _ => {}
        }
    }
    None
}

/// One row of `conversation_summaries`, normalized to what a session row
/// needs.
#[derive(Clone, Debug)]
pub struct ConversationSummary {
    /// `CASCADE_RUN_STATUS_*` as the CLI writes it.
    pub status: String,
    /// Foreground finished but detached work continues — agy's own "the
    /// TUI is quiet but something is still running" bit, which PTY output
    /// cannot see.
    pub not_fully_idle: bool,
    /// The CLI itself marked this conversation terminated.
    pub killed: bool,
    pub title: String,
    pub last_modified_unix: u64,
}

impl ConversationSummary {
    /// The conversation's foreground agent is mid-turn.
    pub fn is_running(&self) -> bool {
        self.status == "CASCADE_RUN_STATUS_RUNNING"
    }
}

/// `YYYY-MM-DD HH:MM:SS.micros+00:00`, the shape agy writes into
/// `last_modified_time`. Parsed rather than string-compared so a caller can
/// bound discovery by spawn time.
fn parse_modified_time(value: &str) -> u64 {
    chrono::DateTime::parse_from_str(value.trim(), "%Y-%m-%d %H:%M:%S%.f%:z")
        .map(|time| time.timestamp().max(0) as u64)
        .unwrap_or(0)
}

fn open_summaries() -> Option<rusqlite::Connection> {
    let path = summaries_db_path()?;
    if !path.exists() {
        return None;
    }
    rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).ok()
}

pub fn conversation_summary(conversation_id: &str) -> Option<ConversationSummary> {
    conversation_summaries(std::slice::from_ref(&conversation_id.to_owned()))
        .remove(conversation_id)
}

/// One batched read for every conversation id the app tracks — the poller
/// calls this once per tick rather than opening the db per session.
pub fn conversation_summaries(ids: &[String]) -> HashMap<String, ConversationSummary> {
    let mut found = HashMap::new();
    let Some(connection) = open_summaries() else {
        return found;
    };
    for id in ids {
        let summary = connection
            .query_row(
                "SELECT status, not_fully_idle, killed, title, last_modified_time \
                 FROM conversation_summaries WHERE conversation_id = ?1",
                [id],
                |row| {
                    Ok(ConversationSummary {
                        status: row.get(0)?,
                        not_fully_idle: row.get(1)?,
                        killed: row.get(2)?,
                        title: row.get(3)?,
                        last_modified_unix: parse_modified_time(&row.get::<_, String>(4)?),
                    })
                },
            )
            .ok();
        if let Some(summary) = summary {
            found.insert(id.clone(), summary);
        }
    }
    found
}

/// The newest conversation in a workspace, used when `last_conversations`
/// has no answer — for example the map entry was rotated by another `agy`
/// run. Rows written before `seen_at` belong to earlier sessions.
pub fn newest_conversation_for_cwd(cwd: &Path, seen_at: u64) -> Option<String> {
    let connection = open_summaries()?;
    // Same realpath caveat as `conversation_id_for_cwd`: the stored URI may
    // carry either spelling of the workspace path.
    let canonical = std::fs::canonicalize(cwd).unwrap_or_else(|_| cwd.to_path_buf());
    let uri = format!("file://{}", canonical.to_string_lossy());
    let uri_as_given = format!("file://{}", cwd.to_string_lossy());
    let mut statement = connection
        .prepare(
            "SELECT conversation_id, last_modified_time FROM conversation_summaries \
             WHERE workspace_uris LIKE ?1 OR workspace_uris LIKE ?2 \
             ORDER BY last_modified_time DESC LIMIT 1",
        )
        .ok()?;
    statement
        .query_row([format!("%{uri}%"), format!("%{uri_as_given}%")], |row| {
            let id: String = row.get(0)?;
            let modified: String = row.get(1)?;
            Ok((id, parse_modified_time(&modified)))
        })
        .ok()
        .filter(|(_, modified)| *modified + SEEN_AT_SLACK_SECS >= seen_at)
        .map(|(id, _)| id)
}

/// The argv for a terminal that runs the CLI's TUI.
///
/// `agy -i "<prompt>"` opens the TUI with the first turn already queued;
/// `agy --conversation <id>` reopens an existing one. `-p`'s greedy prompt
/// argument is the reason the prompt rides as `-i`'s own value rather than
/// a trailing positional.
pub struct AgyLaunch {
    pub args: Vec<String>,
}

impl AgyLaunch {
    pub fn initial_prompt(prompt: &str, model: Option<&str>, effort: Option<&str>) -> Self {
        let mut args = vec!["-i".to_owned(), prompt.to_owned()];
        push_model_options(&mut args, model, effort);
        Self { args }
    }

    pub fn resume(conversation_id: &str, model: Option<&str>, effort: Option<&str>) -> Self {
        let mut args = vec!["--conversation".to_owned(), conversation_id.to_owned()];
        push_model_options(&mut args, model, effort);
        Self { args }
    }

    /// A bare TUI, for a session whose conversation id was never captured.
    /// `agy -c` is deliberately not the fallback: it resumes whatever
    /// conversation the CLI last touched, which may not be this session's.
    pub fn fresh(model: Option<&str>, effort: Option<&str>) -> Self {
        let mut args = Vec::new();
        push_model_options(&mut args, model, effort);
        Self { args }
    }
}

fn push_model_options(args: &mut Vec<String>, model: Option<&str>, effort: Option<&str>) {
    if let Some(model) = model.filter(|model| !model.is_empty()) {
        args.push("--model".to_owned());
        args.push(model.to_owned());
    }
    if let Some(effort) = effort.filter(|effort| !effort.is_empty()) {
        args.push("--effort".to_owned());
        args.push(effort.to_owned());
    }
}
