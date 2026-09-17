//! Muse Code's provider-side session surface: `session/list`,
//! `session/read` history, cold `session/fork`, and the `model/list` catalog.
//!
//! Everything here rides the shared `muse serve` host rather than spawning
//! one-shot `muse` CLIs: listing attaches to the running host when one
//! exists and starts one only when discovery explicitly asks for it.

use std::path::Path;

use anyhow::{anyhow, bail};
use serde_json::{Value, json};

use uuid::Uuid;

use crate::model::{
    AgentTurn, Message, MessageRole, ProviderAgentPreset, ProviderModel, ProviderModelOption,
    ProviderResumeCursor, ProviderSessionHistory, ProviderSessionSummary, TurnStatus,
};
use crate::muse_service::{self, MuseService};

const PAGE_LIMIT: usize = 200;
const MAX_PAGES: usize = 32;

/// The newest Muse sessions, `updatedAt` descending.
///
/// `session/list` is host-global; results are filtered to nothing when the
/// daemon asks with no host running and discovery has not spawned one.
pub fn list_provider_sessions(
    binary: &Path,
    limit: usize,
) -> anyhow::Result<Vec<ProviderSessionSummary>> {
    let Some(service) = muse_service::attached(binary) else {
        // A cold `muse serve` just to enumerate sessions costs a spawn and an
        // initialize handshake; do it — there is no lighter listing surface.
        return list_with_host(muse_service::acquire(binary)?, limit);
    };
    list_with_host(service, limit)
}

fn list_with_host(
    service: MuseService,
    limit: usize,
) -> anyhow::Result<Vec<ProviderSessionSummary>> {
    let mut sessions = Vec::new();
    let mut cursor = Value::Null;
    for _ in 0..MAX_PAGES {
        let result = service
            .call(
                "session/list",
                json!({
                    "cursor": cursor,
                    "limit": PAGE_LIMIT.min(limit.max(1)),
                }),
            )
            .map_err(|error| anyhow!("could not list Muse Code sessions: {}", error.message()))?;
        for session in result
            .get("sessions")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
        {
            if let Some(summary) = session_summary(&session) {
                sessions.push(summary);
            }
            if sessions.len() >= limit {
                return Ok(sessions);
            }
        }
        match result.get("nextCursor").and_then(Value::as_str) {
            Some(next) => cursor = json!(next),
            None => break,
        }
    }
    Ok(sessions)
}

fn session_summary(session: &Value) -> Option<ProviderSessionSummary> {
    let session_id = session.get("sessionId").and_then(Value::as_str)?.to_owned();
    if session_id.is_empty() {
        return None;
    }
    let workspace = session.get("workspaceRoot").and_then(Value::as_str);
    Some(ProviderSessionSummary {
        cursor: ProviderResumeCursor::Muse {
            session_id: session_id.clone(),
            view_cursor: None,
        },
        // Session objects carry no title — the first prompt only exists in
        // the view — so the row falls back to a short id like other
        // title-less providers.
        title: crate::acp_session::session_title(
            ProviderResumeCursor::Muse {
                session_id: session_id.clone(),
                view_cursor: None,
            }
            .provider(),
            None,
            &session_id,
        ),
        cwd: workspace.map(std::path::PathBuf::from).unwrap_or_default(),
        created_at: rfc3339_millis(session.get("createdAt")),
        updated_at: rfc3339_millis(session.get("updatedAt")),
    })
}

fn rfc3339_millis(value: Option<&Value>) -> u64 {
    value
        .and_then(Value::as_str)
        .and_then(|raw| chrono::DateTime::parse_from_rfc3339(raw).ok())
        .map(|time| time.timestamp_millis().max(0) as u64)
        .unwrap_or(0)
}

/// The displayable transcript of a stored Muse session.
///
/// `session/read` serves the folded `history.items` inline (or a snapshot);
/// when it refuses inline history the items are paged in through `view/page`.
pub fn provider_session_history(
    binary: &Path,
    session_id: &str,
    visible_turn_limit: usize,
) -> anyhow::Result<ProviderSessionHistory> {
    let service = match muse_service::attached(binary) {
        Some(service) => service,
        None => muse_service::acquire(binary)?,
    };
    let result = service
        .call(
            "session/read",
            json!({
                "sessionId": session_id,
                // The default serves no items; ask for them inline before
                // falling back to `view/page`.
                "excludeItems": false,
            }),
        )
        .map_err(|error| anyhow!("could not read the Muse Code session: {}", error.message()))?;
    let items = match result.pointer("/history/items").and_then(Value::as_array) {
        Some(items) => items.clone(),
        // `mode: "snapshot"`/`"none"` serve no item array; page the view.
        None => page_items(&service, session_id),
    };
    Ok(items_to_history(&items, visible_turn_limit))
}

/// Replays `item/*` view events into their `item` payloads, in view order.
fn page_items(service: &MuseService, session_id: &str) -> Vec<Value> {
    let mut items = Vec::new();
    let mut cursor: Option<String> = None;
    for _ in 0..MAX_PAGES {
        let mut params = json!({
            "sessionId": session_id,
            "direction": "forward",
            "limit": 1000,
        });
        // `ViewPageParams.cursor` is a plain string — absent means the
        // first page; an explicit null is invalid params here.
        if let Some(cursor) = cursor.as_deref() {
            params["cursor"] = json!(cursor);
        }
        let Ok(result) = service.call("view/page", params) else {
            break;
        };
        let events = result
            .get("events")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        for event in &events {
            match event.get("method").and_then(Value::as_str) {
                Some("item/started") | Some("item/updated") | Some("item/completed") => {
                    if let Some(item) = event.get("params").and_then(|p| p.get("item")) {
                        upsert_item(&mut items, item);
                    }
                }
                _ => {}
            }
        }
        match result.get("nextCursor").and_then(Value::as_str) {
            Some(next) => cursor = Some(next.to_owned()),
            None => break,
        }
    }
    items
}

/// `item/*` events are a revisioned log; the latest revision of an item is
/// the state to display.
fn upsert_item(items: &mut Vec<Value>, item: &Value) {
    let Some(item_id) = item.get("itemId").and_then(Value::as_str) else {
        return;
    };
    if let Some(existing) = items
        .iter_mut()
        .find(|existing| existing.get("itemId").and_then(Value::as_str) == Some(item_id))
    {
        *existing = item.clone();
    } else {
        items.push(item.clone());
    }
}

fn items_to_history(items: &[Value], visible_turn_limit: usize) -> ProviderSessionHistory {
    let mut history = ProviderSessionHistory::default();
    // One turn shell per distinct turnId, in order; the transcript's user and
    // assistant items join the turn they name.
    let mut turn_ids: Vec<String> = Vec::new();
    let mut turn_uuids: Vec<Uuid> = Vec::new();
    for item in items {
        if let Some(turn_id) = item.get("turnId").and_then(Value::as_str)
            && !turn_id.is_empty()
            && !turn_ids.iter().any(|id| id == turn_id)
        {
            turn_ids.push(turn_id.to_owned());
            let turn_uuid = Uuid::new_v4();
            turn_uuids.push(turn_uuid);
            history.turns.push(AgentTurn {
                id: turn_uuid,
                turn_count: history.turns.len() + 1,
                status: TurnStatus::Completed,
                provider_turn_started: true,
                provider_resume_at: None,
                started_at: 0,
                completed_at: Some(0),
                checkpoint: None,
            });
        }
    }
    let first_visible = turn_ids.len().saturating_sub(visible_turn_limit);
    let turn_of = |item: &Value| {
        item.get("turnId")
            .and_then(Value::as_str)
            .and_then(|id| turn_ids.iter().position(|known| known == id))
    };
    for item in items {
        let text = item
            .get("text")
            .and_then(Value::as_str)
            .or_else(|| item.get("displayText").and_then(Value::as_str))
            .unwrap_or_default();
        let index = turn_of(item);
        let visible = index.is_none_or(|index| index >= first_visible);
        if !visible || text.is_empty() {
            continue;
        }
        let role = match item.get("kind").and_then(Value::as_str) {
            Some("userMessage") => MessageRole::User,
            Some("agentMessage") => MessageRole::Assistant,
            _ => continue,
        };
        let message = match index {
            Some(index) => Message::new_for_turn(role, text.to_owned(), turn_uuids[index]),
            None => Message::new(role, text.to_owned()),
        };
        history.messages.push(message);
    }
    history
}

/// One finished turn in view order. `turn/completed` fires for every
/// terminal state — failed and cancelled turns count toward Goddard's
/// provider-turn indexes, but only `completed` is a legal `session/fork`
/// boundary.
#[derive(Clone)]
pub(crate) struct FinishedTurn {
    pub(crate) turn_id: String,
    pub(crate) completed: bool,
}

/// The `session/fork` cut point for "keep the first `retained` finished
/// turns": the nearest `completed` turn at or before that edge. A failed
/// or cancelled edge cannot be a boundary, so the cut snaps back to the
/// last completed turn — never keeps a turn meant to be dropped.
pub(crate) fn fork_boundary_id(finished: &[FinishedTurn], retained: usize) -> Option<String> {
    finished
        .get(..retained)?
        .iter()
        .rposition(|turn| turn.completed)
        .map(|index| finished[index].turn_id.clone())
}

/// Branches a stored session, keeping its first `retained_turns` completed
/// turns. Cold path: the daemon calls this when no live driver can fork.
pub fn fork_session_at_turn(
    binary: &Path,
    session_id: &str,
    retained_turns: usize,
) -> anyhow::Result<ProviderResumeCursor> {
    let service = match muse_service::attached(binary) {
        Some(service) => service,
        None => muse_service::acquire(binary)?,
    };
    let finished = finished_turns(&service, session_id)?;
    if retained_turns > finished.len() {
        bail!(
            "Muse Code has only {} turns, but Goddard needs {retained_turns}",
            finished.len()
        );
    }
    let Some(last_turn_id) = fork_boundary_id(&finished, retained_turns) else {
        bail!("Muse Code cannot fork to before its first completed turn");
    };
    let result = service
        .call(
            "session/fork",
            json!({
                "commandId": service.mint_command_id(),
                "sessionId": session_id,
                "cutPoint": { "lastTurnId": last_turn_id },
                "excludeItems": true,
            }),
        )
        .map_err(|error| anyhow!("could not fork the Muse Code session: {}", error.message()))?;
    let fork_id = result
        .pointer("/session/sessionId")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("Muse Code returned no forked session ID"))?;
    Ok(ProviderResumeCursor::Muse {
        session_id: fork_id.to_owned(),
        view_cursor: result
            .get("viewCursor")
            .and_then(Value::as_str)
            .map(str::to_owned),
    })
}

/// Every turn that finished, in view order, from `turn/completed` events —
/// all terminals, so the list lines up with Goddard's provider-turn count.
pub(crate) fn finished_turns(
    service: &MuseService,
    session_id: &str,
) -> anyhow::Result<Vec<FinishedTurn>> {
    let mut turns: Vec<FinishedTurn> = Vec::new();
    let mut cursor: Option<String> = None;
    for _ in 0..MAX_PAGES {
        let mut params = json!({
            "sessionId": session_id,
            "direction": "forward",
            "limit": 1000,
        });
        if let Some(cursor) = cursor.as_deref() {
            params["cursor"] = json!(cursor);
        }
        let result = service.call("view/page", params).map_err(|error| {
            anyhow!(
                "could not read the Muse Code session view: {}",
                error.message()
            )
        })?;
        for event in result
            .get("events")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
        {
            if event.get("method").and_then(Value::as_str) == Some("turn/completed")
                && let Some(turn_id) = event.pointer("/params/turnId").and_then(Value::as_str)
            {
                let completed = event.pointer("/params/terminal").and_then(Value::as_str)
                    == Some("completed");
                if let Some(turn) = turns.iter_mut().find(|turn| turn.turn_id == turn_id) {
                    turn.completed |= completed;
                } else {
                    turns.push(FinishedTurn {
                        turn_id: turn_id.to_owned(),
                        completed,
                    });
                }
            }
        }
        match result.get("nextCursor").and_then(Value::as_str) {
            Some(next) => cursor = Some(next.to_owned()),
            None => break,
        }
    }
    Ok(turns)
}

/// The live `model/list` catalog, or the last-good cache the catalog layer
/// falls back to when no host is running.
pub(crate) fn discover_catalog(
    binary: &Path,
) -> (Vec<ProviderModel>, Option<Vec<ProviderAgentPreset>>) {
    let Some(service) = muse_service::attached(binary) else {
        return (Vec::new(), None);
    };
    let result = service.call("model/list", json!({}));
    let models = result
        .ok()
        .and_then(|result| result.get("models").and_then(Value::as_array).cloned())
        .unwrap_or_default()
        .iter()
        .filter_map(catalog_model)
        .collect();
    (models, None)
}

/// One catalog row. Muse models accept a per-turn reasoning effort, so every
/// model exposes the same closed MSP ladder.
fn catalog_model(entry: &Value) -> Option<ProviderModel> {
    let model_id = entry.get("modelId").and_then(Value::as_str)?.to_owned();
    if model_id.is_empty() {
        return None;
    }
    let name = entry
        .get("displayLabel")
        .and_then(Value::as_str)
        .filter(|label| !label.trim().is_empty())
        .unwrap_or(&model_id)
        .to_owned();
    let mut model = ProviderModel::new(model_id, name);
    if let Some(provider) = entry.get("providerId").and_then(Value::as_str) {
        model = model.sub_provider(provider);
    }
    if entry.get("isDefault").and_then(Value::as_bool) == Some(true) {
        model = model.default();
    }
    model.reasoning_efforts = ["low", "medium", "high", "xhigh", "ultra"]
        .into_iter()
        .map(|id| ProviderModelOption::new(id, id))
        .collect();
    Some(model)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use uuid::Uuid;

    use super::*;
    use crate::muse_service::test_support::fake_muse;

    /// A cut snaps back to the last `completed` turn at or before the edge:
    /// failed and cancelled turns count toward the index but cannot be a
    /// boundary, and nothing retained means no cut at all.
    #[test]
    fn fork_boundary_snaps_back_to_completed() {
        let turns = vec![
            FinishedTurn {
                turn_id: "t1".into(),
                completed: true,
            },
            FinishedTurn {
                turn_id: "t2".into(),
                completed: false,
            },
            FinishedTurn {
                turn_id: "t3".into(),
                completed: true,
            },
        ];
        assert_eq!(fork_boundary_id(&turns, 3).as_deref(), Some("t3"));
        assert_eq!(fork_boundary_id(&turns, 2).as_deref(), Some("t1"));
        assert_eq!(fork_boundary_id(&turns, 1).as_deref(), Some("t1"));
        assert_eq!(fork_boundary_id(&turns, 0), None);
        assert_eq!(fork_boundary_id(&turns, 4), None);
    }

    /// `session/read` may refuse inline items; the fallback then pages the
    /// view — whose `cursor` param must be absent, not null, on page one.
    #[test]
    fn history_falls_back_to_paging_the_view() {
        let directory =
            std::env::temp_dir().join(format!("waku-muse-session-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap();
        let binary = fake_muse(&directory);

        let history = provider_session_history(&binary, "s1", 50).unwrap();

        let texts: Vec<&str> = history
            .messages
            .iter()
            .map(|message| message.content.as_str())
            .collect();
        assert!(texts.contains(&"first prompt"));
        assert!(texts.contains(&"first answer"));
        assert!(!directory.join("violations.log").exists());
    }

    /// The cold fork pages `turn/completed` events to find the boundary and
    /// returns the forked session's resume cursor.
    #[test]
    fn cold_fork_pages_the_view_for_a_boundary() {
        let directory =
            std::env::temp_dir().join(format!("waku-muse-session-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap();
        let binary = fake_muse(&directory);

        let cursor = fork_session_at_turn(&binary, "s1", 1).unwrap();
        let ProviderResumeCursor::Muse {
            session_id,
            view_cursor,
        } = cursor
        else {
            panic!("expected a Muse cursor");
        };
        assert_eq!(session_id, "fork-1");
        assert_eq!(view_cursor.as_deref(), Some("cf"));
        assert!(!directory.join("violations.log").exists());
    }
}
