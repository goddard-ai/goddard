//! GitHub Copilot session discovery and transcript replay from the CLI's
//! on-disk session store.
//!
//! The Copilot CLI persists every session under `~/.copilot/session-state/
//! <id>/`; `events.jsonl` there is the complete event log — the same stream
//! the SDK broadcasts live. Reading the file directly keeps the picker cheap:
//! no `copilot` process launches just to enumerate or import sessions.

use std::fs;
use std::path::PathBuf;

use serde_json::Value;
use uuid::Uuid;

use crate::model::{
    AgentTurn, Message, MessageRole, ProviderKind, ProviderResumeCursor,
    ProviderSessionHistory, ProviderSessionSummary, TurnStatus,
};

/// `$COPILOT_HOME/session-state`, defaulting to `~/.copilot/session-state`.
fn sessions_directory() -> Option<PathBuf> {
    let root = std::env::var_os("COPILOT_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|home| home.join(".copilot")))?;
    Some(root.join("session-state"))
}

fn events_path(session_id: &str) -> Option<PathBuf> {
    // Session ids name a directory verbatim; anything containing a separator
    // or parent traversal cannot be one.
    if session_id.is_empty()
        || session_id == ".."
        || session_id.chars().any(|ch| matches!(ch, '/' | '\\' | '\0'))
    {
        return None;
    }
    sessions_directory().map(|directory| directory.join(session_id).join("events.jsonl"))
}

fn read_events(session_id: &str) -> Option<Vec<Value>> {
    let path = events_path(session_id)?;
    let content = fs::read_to_string(path).ok()?;
    Some(
        content
            .lines()
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect(),
    )
}

fn event_type(event: &Value) -> Option<&str> {
    event.get("type").and_then(Value::as_str)
}

fn event_timestamp(event: &Value) -> Option<u64> {
    event
        .get("timestamp")
        .and_then(Value::as_str)
        .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
        .and_then(|value| u64::try_from(value.timestamp()).ok())
}

/// A title the user wrote beats the CLI's generated `session.title_changed`
/// only when no title exists — the generated one is a real session label, so
/// it wins; the first prompt stands in when it never ran.
fn session_title(events: &[Value], session_id: &str) -> String {
    let title = events
        .iter()
        .rev()
        .find(|event| event_type(event) == Some("session.title_changed"))
        .and_then(|event| {
            event
                .pointer("/data/title")
                .and_then(Value::as_str)
                .map(str::to_owned)
        });
    let title = title.or_else(|| {
        events
            .iter()
            .find(|event| event_type(event) == Some("user.message"))
            .and_then(|event| {
                event
                    .pointer("/data/content")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            })
            .map(|content| {
                content
                    .lines()
                    .next()
                    .unwrap_or_default()
                    .trim()
                    .chars()
                    .take(80)
                    .collect::<String>()
            })
            .filter(|title| !title.is_empty())
    });
    crate::acp_session::session_title(ProviderKind::Copilot, title.as_deref(), session_id)
}

pub fn list_provider_sessions(limit: usize) -> anyhow::Result<Vec<ProviderSessionSummary>> {
    if limit == 0 {
        return Ok(Vec::new());
    }
    let Some(directory) = sessions_directory() else {
        return Ok(Vec::new());
    };
    if !directory.is_dir() {
        return Ok(Vec::new());
    }
    let mut sessions = Vec::new();
    for entry in fs::read_dir(&directory)? {
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => continue,
        };
        if !entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            continue;
        }
        let session_id = entry.file_name().to_string_lossy().into_owned();
        let events = match read_events(&session_id) {
            Some(events) => events,
            None => continue,
        };
        let start = events
            .iter()
            .find(|event| event_type(event) == Some("session.start"));
        let cwd = start
            .and_then(|event| event.pointer("/data/context/cwd"))
            .and_then(Value::as_str)
            .map(PathBuf::from)
            .or_else(dirs::home_dir)
            .unwrap_or_default();
        let created_at = start
            .and_then(event_timestamp)
            .or_else(|| events.first().and_then(event_timestamp))
            .unwrap_or_default();
        let updated_at = events
            .iter()
            .rev()
            .find_map(event_timestamp)
            .unwrap_or(created_at);
        sessions.push(ProviderSessionSummary {
            cursor: ProviderResumeCursor::Copilot {
                session_id: session_id.clone(),
            },
            title: session_title(&events, &session_id),
            cwd,
            created_at,
            updated_at,
        });
    }
    sessions.sort_by_key(|a| std::cmp::Reverse(a.updated_at));
    sessions.truncate(limit);
    Ok(sessions)
}

/// Replays the user-visible transcript out of `events.jsonl`. A `user.message`
/// opens a turn; root-agent `assistant.message` events append to it. The event
/// ids populate `provider_resume_at` — they are exactly the boundaries
/// `session.fork`'s `to_event_id` accepts.
pub fn provider_session_history(
    session_id: &str,
    visible_turn_limit: usize,
) -> anyhow::Result<ProviderSessionHistory> {
    let mut history = ProviderSessionHistory::default();
    if visible_turn_limit == 0 {
        return Ok(history);
    }
    let Some(events) = read_events(session_id) else {
        return Ok(history);
    };
    for event in &events {
        if event.get("agentId").is_some() {
            // A sub-agent's transcript belongs to its own run, not the main
            // conversation.
            continue;
        }
        let timestamp = event_timestamp(event).unwrap_or_else(crate::model::unix_time);
        match event_type(event) {
            Some("user.message") => {
                let turn_id = Uuid::new_v4();
                history.turns.push(AgentTurn {
                    id: turn_id,
                    turn_count: history.turns.len() + 1,
                    status: TurnStatus::Completed,
                    provider_turn_started: true,
                    provider_resume_at: event
                        .get("id")
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                    started_at: timestamp,
                    completed_at: Some(timestamp),
                    checkpoint: None,
                });
                let content = event
                    .pointer("/data/content")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .trim();
                if !content.is_empty() {
                    let mut message =
                        Message::new_for_turn(MessageRole::User, content.to_owned(), turn_id);
                    message.created_at = timestamp;
                    history.messages.push(message);
                }
            }
            Some("assistant.message") => {
                let Some(turn) = history.turns.last_mut() else {
                    continue;
                };
                if let Some(id) = event.get("id").and_then(Value::as_str) {
                    turn.provider_resume_at = Some(id.to_owned());
                }
                turn.completed_at =
                    Some(turn.completed_at.unwrap_or(timestamp).max(timestamp));
                let content = event
                    .pointer("/data/content")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                if content.is_empty() {
                    continue;
                }
                if let Some(previous) = history.messages.last_mut().filter(|message| {
                    message.turn_id == Some(turn.id) && message.role == MessageRole::Assistant
                }) {
                    if !previous.content.is_empty() {
                        previous.content.push_str("\n\n");
                    }
                    previous.content.push_str(content);
                    previous.created_at = previous.created_at.max(timestamp);
                } else {
                    let mut message =
                        Message::new_for_turn(MessageRole::Assistant, content.to_owned(), turn.id);
                    message.created_at = timestamp;
                    history.messages.push(message);
                }
            }
            _ => {}
        }
    }
    if history.turns.len() > visible_turn_limit {
        let retained = history
            .turns
            .iter()
            .rev()
            .take(visible_turn_limit)
            .map(|turn| turn.id)
            .collect::<std::collections::HashSet<_>>();
        history
            .messages
            .retain(|message| message.turn_id.is_some_and(|id| retained.contains(&id)));
    }
    Ok(history)
}
