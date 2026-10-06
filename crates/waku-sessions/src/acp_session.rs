//! Provider-neutral ACP session discovery and transcript replay.
//!
//! ACP agents own their storage migrations and visible-history projection. A
//! one-shot `session/list` or `session/load` therefore stays more accurate than
//! reading their private stores, while typed updates let Goddard discard private
//! reasoning and provider-only tool records by construction.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Context as _;
use serde_json::Value;
use uuid::Uuid;

use crate::model::{
    AgentTurn, Message, MessageRole, ProviderKind, ProviderSessionCatalogStatus,
    ProviderSessionHistory, ProviderSessionSummary, TurnStatus,
};

/// A provider's session catalog plus why it may be empty.
pub struct ProviderSessionCatalog {
    pub sessions: Vec<ProviderSessionSummary>,
    pub status: ProviderSessionCatalogStatus,
}

impl ProviderSessionCatalog {
    pub fn unsupported() -> Self {
        Self {
            sessions: Vec::new(),
            status: ProviderSessionCatalogStatus::Unsupported,
        }
    }
}

impl From<Vec<ProviderSessionSummary>> for ProviderSessionCatalog {
    fn from(sessions: Vec<ProviderSessionSummary>) -> Self {
        Self {
            sessions,
            status: ProviderSessionCatalogStatus::Ready,
        }
    }
}

/// Empty, app-owned cwd for provider catalog subprocesses.
///
/// Agent CLIs may index their process cwd during startup. Launching a catalog
/// from `$HOME` therefore makes a harmless session-list request recursively
/// touch Desktop, Documents, Downloads, Photos, and other macOS TCC locations.
/// Keeping discovery in an isolated temp directory prevents that implicit
/// workspace scan; actual session loading still receives the selected cwd.
pub fn catalog_working_directory() -> anyhow::Result<PathBuf> {
    let directory = std::env::temp_dir().join(format!(
        "waku-provider-session-catalog-{}",
        std::process::id()
    ));
    fs::create_dir_all(&directory).with_context(|| {
        format!(
            "could not create provider-session catalog directory {}",
            directory.display()
        )
    })?;
    Ok(directory)
}

/// The cwd a resumed session can actually launch in. When the recorded
/// directory is gone, fall back to its nearest existing ancestor, then to
/// the isolated catalog directory.
pub fn resume_working_directory(cwd: &Path) -> PathBuf {
    if cwd.is_dir() {
        return cwd.to_path_buf();
    }
    cwd.ancestors()
        .skip(1)
        .find(|candidate| candidate.is_dir())
        .map(Path::to_path_buf)
        .or_else(|| catalog_working_directory().ok())
        .unwrap_or_else(std::env::temp_dir)
}

pub fn timestamp(value: Option<&str>) -> u64 {
    value
        .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
        .and_then(|value| u64::try_from(value.timestamp()).ok())
        .unwrap_or_default()
}

/// The wall-clock stamp a provider attaches to a replayed update, when it
/// exposes one. Devin stamps every `session/load` update under
/// `_meta["cognition.ai/timestamp"]`; other providers may use a plain
/// `_meta.timestamp`. Zero means the provider gave none.
fn update_timestamp(update: &Value) -> Option<u64> {
    let meta = update.get("_meta")?;
    ["cognition.ai/timestamp", "timestamp"]
        .into_iter()
        .find_map(|key| meta.get(key).and_then(Value::as_str))
        .map(|value| timestamp(Some(value)))
        .filter(|at| *at > 0)
}

pub fn session_title(provider: ProviderKind, title: Option<&str>, session_id: &str) -> String {
    title
        .map(str::trim)
        .filter(|title| !title.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| {
            let short_id = session_id.chars().take(8).collect::<String>();
            format!("{} session {short_id}", provider.short_name())
        })
}

pub fn history_from_updates(
    provider: ProviderKind,
    updates: &[Value],
    fallback_timestamp: Option<u64>,
) -> ProviderSessionHistory {
    let mut history = ProviderSessionHistory::default();
    let mut saw_agent_activity = true;
    let fallback = fallback_timestamp
        .filter(|at| *at > 0)
        .unwrap_or_else(crate::model::unix_time);

    for update in updates {
        let at = update_timestamp(update);
        let kind = update.get("sessionUpdate").and_then(Value::as_str);
        if kind == Some("user_message_chunk") {
            let Some(text) = update
                .pointer("/content/text")
                .and_then(Value::as_str)
                .filter(|text| !text.is_empty())
            else {
                continue;
            };
            if history.turns.is_empty() || saw_agent_activity {
                let turn_id = Uuid::new_v4();
                history.turns.push(AgentTurn {
                    id: turn_id,
                    turn_count: history.turns.len() + 1,
                    status: TurnStatus::Completed,
                    provider_turn_started: true,
                    provider_resume_at: None,
                    interruption: None,
                    started_at: at.unwrap_or(fallback),
                    completed_at: None,
                    checkpoint: None,
                });
                let mut message = Message::new_for_turn(MessageRole::User, text, turn_id);
                message.created_at = at.unwrap_or(fallback);
                history.messages.push(message);
                saw_agent_activity = false;
            } else if let Some(message) = history.messages.last_mut().filter(|message| {
                message.role == MessageRole::User
                    && message.turn_id == history.turns.last().map(|turn| turn.id)
            }) {
                message.content.push_str(text);
            }
            if let (Some(at), Some(turn)) = (at, history.turns.last_mut()) {
                turn.completed_at = Some(turn.completed_at.unwrap_or(0).max(at));
            }
            continue;
        }

        if matches!(
            kind,
            Some(
                "agent_message_chunk"
                    | "agent_thought_chunk"
                    | "tool_call"
                    | "tool_call_update"
                    | "plan"
            )
        ) {
            saw_agent_activity = true;
            if let (Some(at), Some(turn)) = (at, history.turns.last_mut()) {
                turn.completed_at = Some(turn.completed_at.unwrap_or(0).max(at));
            }
        }
        if kind != Some("agent_message_chunk") {
            continue;
        }
        let Some(text) = update
            .pointer("/content/text")
            .and_then(Value::as_str)
            .filter(|text| !text.is_empty())
        else {
            continue;
        };
        if provider == ProviderKind::Fx
            && (text.starts_with("[context] ") || text.starts_with("skill discovery warning: "))
        {
            continue;
        }
        let Some(turn_id) = history.turns.last().map(|turn| turn.id) else {
            continue;
        };
        if let Some(message) = history.messages.last_mut().filter(|message| {
            message.role == MessageRole::Assistant && message.turn_id == Some(turn_id)
        }) {
            message.content.push_str(text);
        } else {
            let mut message = Message::new_for_turn(MessageRole::Assistant, text, turn_id);
            message.created_at = at.unwrap_or(fallback);
            history.messages.push(message);
        }
    }
    for turn in &mut history.turns {
        turn.completed_at = Some(
            turn.completed_at
                .filter(|at| *at > 0)
                .unwrap_or(turn.started_at)
                .max(turn.started_at),
        );
    }
    history
}

pub fn retain_recent_messages(history: &mut ProviderSessionHistory, limit: usize) {
    let retained = history
        .turns
        .iter()
        .rev()
        .take(limit)
        .map(|turn| turn.id)
        .collect::<HashSet<_>>();
    history
        .messages
        .retain(|message| message.turn_id.is_some_and(|id| retained.contains(&id)));
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn catalog_processes_use_an_isolated_temp_working_directory() {
        let directory = catalog_working_directory().unwrap();
        assert!(directory.starts_with(std::env::temp_dir()));
        assert!(directory.is_dir());
        if let Some(home) = dirs::home_dir() {
            assert_ne!(directory, home);
            for protected in [
                "Desktop",
                "Documents",
                "Downloads",
                "Movies",
                "Music",
                "Pictures",
            ] {
                assert!(!directory.starts_with(home.join(protected)));
            }
        }
    }

    #[test]
    fn imports_only_visible_acp_chunks_and_preserves_empty_turn_shells() {
        let history = history_from_updates(
            ProviderKind::Kimi,
            &[
                json!({"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"one"}}),
                json!({"sessionUpdate":"agent_thought_chunk","content":{"type":"text","text":"private"}}),
                json!({"sessionUpdate":"tool_call","toolCallId":"call-1","title":"read"}),
                json!({"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"done"}}),
                json!({"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"two"}}),
                json!({"sessionUpdate":"tool_call","toolCallId":"call-2","title":"bash"}),
            ],
            None,
        );

        assert_eq!(history.turns.len(), 2);
        assert_eq!(history.messages.len(), 3);
        assert_eq!(history.messages[0].content, "one");
        assert_eq!(history.messages[1].content, "done");
        assert_eq!(history.messages[2].content, "two");
        assert!(
            history
                .messages
                .iter()
                .all(|message| !message.content.contains("private"))
        );
    }

    #[test]
    fn resume_working_directory_keeps_existing_and_falls_back_to_ancestors() {
        let existing = std::env::temp_dir();
        assert_eq!(resume_working_directory(&existing), existing);

        let missing = existing.join("waku-no-such-dir").join("deeper");
        assert_eq!(resume_working_directory(&missing), existing);

        // No surviving ancestor: falls back to a directory that exists.
        let unresolvable = PathBuf::from("/waku-no-such-root/branch/leaf");
        assert!(resume_working_directory(&unresolvable).is_dir());
    }

    #[test]
    fn joins_consecutive_content_blocks_without_inventing_turns() {
        let history = history_from_updates(
            ProviderKind::Kimi,
            &[
                json!({"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"hello "}}),
                json!({"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"world"}}),
                json!({"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"part 1"}}),
                json!({"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":" + part 2"}}),
            ],
            None,
        );

        assert_eq!(history.turns.len(), 1);
        assert_eq!(history.messages[0].content, "hello world");
        assert_eq!(history.messages[1].content, "part 1 + part 2");
    }

    #[test]
    fn stamps_replayed_turns_and_messages_with_provider_timestamps() {
        let history = history_from_updates(
            ProviderKind::Devin,
            &[
                json!({"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"one"},
                    "_meta":{"cognition.ai/timestamp":"2026-09-28T18:17:15.948067+00:00"}}),
                json!({"sessionUpdate":"tool_call","toolCallId":"call-1","title":"read",
                    "_meta":{"cognition.ai/timestamp":"2026-09-28T18:18:28.769930+00:00"}}),
                json!({"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"done"},
                    "_meta":{"cognition.ai/timestamp":"2026-09-28T18:19:35.759347+00:00"}}),
                json!({"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"two"},
                    "_meta":{"cognition.ai/timestamp":"2026-09-28T18:20:46.611494+00:00"}}),
            ],
            None,
        );

        let started = timestamp(Some("2026-09-28T18:17:15.948067+00:00"));
        let finished = timestamp(Some("2026-09-28T18:19:35.759347+00:00"));
        assert_eq!(history.turns[0].started_at, started);
        assert_eq!(history.turns[0].completed_at, Some(finished));
        assert_eq!(history.messages[0].created_at, started);
        assert_eq!(history.messages[1].created_at, finished);
        assert_eq!(
            history.turns[1].started_at,
            timestamp(Some("2026-09-28T18:20:46.611494+00:00"))
        );
        assert_eq!(
            history.turns[1].completed_at,
            Some(history.turns[1].started_at)
        );
    }

    #[test]
    fn unstamped_updates_fall_back_to_the_catalog_timestamp() {
        let history = history_from_updates(
            ProviderKind::Goose,
            &[
                json!({"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"one"}}),
                json!({"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"done"}}),
            ],
            Some(1_700_000_000),
        );

        assert_eq!(history.turns[0].started_at, 1_700_000_000);
        assert_eq!(history.turns[0].completed_at, Some(1_700_000_000));
        assert_eq!(history.messages[0].created_at, 1_700_000_000);
        assert_eq!(history.messages[1].created_at, 1_700_000_000);
    }

    #[test]
    fn unstamped_updates_without_a_fallback_never_report_the_epoch() {
        let history = history_from_updates(
            ProviderKind::Goose,
            &[json!({"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"one"}})],
            None,
        );

        assert!(history.turns[0].started_at > 0);
        assert!(history.turns[0].completed_at.unwrap() > 0);
    }
}
