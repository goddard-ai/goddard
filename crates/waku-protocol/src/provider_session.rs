use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::model::{AgentSession, ProviderResumeCursor};

/// Daemon-host native-session operation used when no live driver can fork.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(tag = "provider", rename_all = "camelCase")]
pub enum ProviderSessionForkRequest {
    Claude {
        session_id: String,
        resume_at: Option<String>,
        turn_count: usize,
        title: String,
    },
    Amp {
        binary: PathBuf,
        cwd: PathBuf,
        thread_id: String,
        fork_context: Option<String>,
        turn_count: usize,
    },
    Cursor {
        source: AgentSession,
        turn_count: usize,
    },
    /// OpenCode sessions carry their own `location`, so there is no server
    /// working directory to fork against; `binary` only lets the cold path
    /// reach the adopted background service.
    #[serde(alias = "openCode2")]
    OpenCode {
        binary: PathBuf,
        session_id: String,
        turn_count: usize,
    },
    Grok {
        binary: PathBuf,
        cwd: PathBuf,
        session_id: String,
        turn_count: usize,
    },
    /// `sessions.fork` is a live RPC, so `binary` launches the throwaway
    /// client that issues it; `turn_count` is the retained provider-turn
    /// count the daemon resolves to the fork's `to_event_id` boundary.
    Copilot {
        binary: PathBuf,
        cwd: PathBuf,
        session_id: String,
        turn_count: usize,
        title: String,
    },
    /// One `muse serve` hosts every workspace, so a cold fork needs only the
    /// binary to reach it and the source session id to cut.
    Muse {
        binary: PathBuf,
        session_id: String,
        turn_count: usize,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSessionFork {
    pub cursor: ProviderResumeCursor,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub message_ids: HashMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_resume_at: Option<String>,
}

/// A background history read may append only to the idle transcript it started
/// from. Metadata is deliberately excluded: renaming a task must survive a refresh.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodexHistoryRefresh {
    thread_id: String,
    turns: Vec<(uuid::Uuid, crate::model::TurnStatus, bool)>,
    message_count: usize,
    last_message_id: Option<uuid::Uuid>,
    updated_at: u64,
    runtime_cursor: Option<crate::model::RuntimeEventCursor>,
}

impl CodexHistoryRefresh {
    pub fn capture(session: &AgentSession) -> Option<Self> {
        if !session.detail_loaded
            || session.incognito
            || session.status.is_busy()
            || session
                .turns
                .iter()
                .any(|turn| turn.status == crate::model::TurnStatus::Running)
        {
            return None;
        }
        let ProviderResumeCursor::Codex { thread_id } = session.provider_cursor.as_ref()? else {
            return None;
        };
        if session.provider != crate::model::ProviderKind::Codex {
            return None;
        }
        Some(Self {
            thread_id: thread_id.clone(),
            turns: session
                .turns
                .iter()
                .map(|t| (t.id, t.status, t.provider_turn_started))
                .collect(),
            message_count: session.messages.len(),
            last_message_id: session.messages.last().map(|m| m.id),
            updated_at: session.updated_at,
            runtime_cursor: session.runtime_event_cursor,
        })
    }

    pub fn thread_id(&self) -> &str {
        &self.thread_id
    }

    pub fn still_current(&self, session: &AgentSession) -> bool {
        Self::capture(session).as_ref() == Some(self)
    }
}

/// Preserve Goddard's richer existing turns (whose ids differ from Codex ids)
/// and append the native suffix by provider turn ordinal. Locally rejected
/// submissions do not consume a provider turn. Never import an unfinished turn:
/// its later completion would otherwise be skipped on the next refresh.
pub fn append_codex_history(
    session: &mut AgentSession,
    history: &crate::model::ProviderSessionHistory,
) -> bool {
    if CodexHistoryRefresh::capture(session).is_none() {
        return false;
    }
    let known = session
        .turns
        .iter()
        .filter(|t| t.provider_turn_started)
        .count();
    let new_turns: Vec<_> = history
        .turns
        .iter()
        .skip(known)
        .take_while(|t| t.completed_at.is_some())
        .collect();
    if new_turns.is_empty() {
        return false;
    }
    let ids: std::collections::HashSet<_> = new_turns.iter().map(|t| t.id).collect();
    for turn in new_turns {
        let mut turn = turn.clone();
        turn.turn_count = session.turns.len() + 1;
        session.updated_at = session
            .updated_at
            .max(turn.completed_at.unwrap_or(turn.started_at));
        session.turns.push(turn);
    }
    session.messages.extend(
        history
            .messages
            .iter()
            .filter(|m| m.turn_id.is_some_and(|id| ids.contains(&id)))
            .cloned(),
    );
    session.last_reply_at = Some(session.updated_at);
    true
}

#[cfg(test)]
mod history_tests {
    use super::*;
    use crate::model::{
        AgentTurn, Message, MessageRole, ProviderKind, ProviderSessionHistory, SessionStatus,
        TurnStatus,
    };
    use uuid::Uuid;

    fn session() -> AgentSession {
        let mut session = AgentSession::new(Uuid::new_v4(), ProviderKind::Codex);
        session.provider_cursor = Some(ProviderResumeCursor::Codex {
            thread_id: "thread".into(),
        });
        session.begin_turn("from Goddard");
        session.mark_active_turn_provider_started();
        session.push_message(MessageRole::Assistant, "original answer");
        session.turns[0].status = TurnStatus::Completed;
        session.turns[0].completed_at = Some(1);
        session.status = SessionStatus::Idle;
        session
    }

    fn native_history() -> ProviderSessionHistory {
        let mut history = ProviderSessionHistory::default();
        for (index, prompt) in ["from Goddard", "from CLI", "second CLI prompt"]
            .iter()
            .enumerate()
        {
            let id = Uuid::new_v4();
            history.turns.push(AgentTurn {
                id,
                turn_count: index + 1,
                status: TurnStatus::Completed,
                provider_turn_started: true,
                provider_resume_at: None,
                interruption: None,
                started_at: index as u64 + 1,
                completed_at: Some(index as u64 + 2),
                checkpoint: None,
            });
            history
                .messages
                .push(Message::new_for_turn(MessageRole::User, *prompt, id));
            history.messages.push(Message::new_for_turn(
                MessageRole::Assistant,
                "CLI answer",
                id,
            ));
        }
        history
    }

    #[test]
    fn reopening_appends_cli_turns_once_and_preserves_goddard_identity_and_history() {
        let mut session = session();
        let original = serde_json::to_value(&session).unwrap();
        let original_message_ids: Vec<_> = session.messages.iter().map(|m| m.id).collect();
        let history = native_history();
        assert_ne!(session.turns[0].id, history.turns[0].id);
        assert!(append_codex_history(&mut session, &history));
        assert_eq!(session.turns.len(), 3);
        assert_eq!(session.messages.len(), 6);
        assert_eq!(session.messages[2].content, "from CLI");
        assert_eq!(session.messages[4].content, "second CLI prompt");
        assert_eq!(
            session.messages[..2]
                .iter()
                .map(|m| m.id)
                .collect::<Vec<_>>(),
            original_message_ids
        );
        assert_eq!(
            serde_json::to_value(&session.turns[0]).unwrap(),
            original["turns"][0]
        );
        assert_eq!(serde_json::to_value(session.id).unwrap(), original["id"]);
        assert_eq!(
            serde_json::to_value(session.project_id).unwrap(),
            original["project_id"]
        );
        assert!(!append_codex_history(&mut session, &history));
        assert_eq!(session.messages.len(), 6);
    }

    #[test]
    fn refresh_rejects_a_racing_prompt_or_provider_switch_but_preserves_metadata() {
        let mut session = session();
        let baseline = CodexHistoryRefresh::capture(&session).unwrap();
        session.title = "renamed task".into();
        assert!(baseline.still_current(&session));
        session.begin_turn("racing prompt");
        assert!(!baseline.still_current(&session));
        assert!(!append_codex_history(&mut session, &native_history()));
        session.status = SessionStatus::Idle;
        assert!(!baseline.still_current(&session));
        session.provider_cursor = Some(ProviderResumeCursor::Codex {
            thread_id: "different thread".into(),
        });
        assert!(!baseline.still_current(&session));
    }

    #[test]
    fn local_rejected_turns_do_not_skip_cli_turns_and_in_progress_turns_wait_for_completion() {
        let mut session = session();
        session.begin_turn("locally rejected");
        session.status = SessionStatus::Failed;
        session.turns[1].status = TurnStatus::Failed;
        let mut history = native_history();
        history.turns[2].completed_at = None;
        assert!(append_codex_history(&mut session, &history));
        assert_eq!(session.turns.len(), 3);
        assert_eq!(session.turns[2].turn_count, 3);
        assert_eq!(
            session.messages.last().unwrap().turn_id,
            Some(history.turns[1].id)
        );
        assert!(!append_codex_history(&mut session, &history));
        history.turns[2].completed_at = Some(4);
        assert!(append_codex_history(&mut session, &history));
        assert_eq!(session.turns.len(), 4);
        assert_eq!(session.turns[3].turn_count, 4);
    }
}
