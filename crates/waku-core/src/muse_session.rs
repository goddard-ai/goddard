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
    let mut cursor = Value::Null;
    for _ in 0..MAX_PAGES {
        let Ok(result) = service.call(
            "view/page",
            json!({
                "sessionId": session_id,
                "cursor": cursor,
                "direction": "forward",
                "limit": 1000,
            }),
        ) else {
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
            Some(next) => cursor = json!(next),
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
    let completed = completed_turn_ids(&service, session_id)?;
    if retained_turns > completed.len() {
        bail!(
            "Muse Code has only {} completed turns, but Goddard needs {retained_turns}",
            completed.len()
        );
    }
    let Some(last_turn_id) = completed.get(retained_turns.wrapping_sub(1)) else {
        bail!("Muse Code cannot fork to before its first turn");
    };
    if retained_turns == 0 {
        bail!("Muse Code cannot fork to before its first turn");
    }
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

/// Completed turn ids in order, read from `turn/completed` view events.
fn completed_turn_ids(service: &MuseService, session_id: &str) -> anyhow::Result<Vec<String>> {
    let mut ids = Vec::new();
    let mut cursor = Value::Null;
    for _ in 0..MAX_PAGES {
        let result = service
            .call(
                "view/page",
                json!({
                    "sessionId": session_id,
                    "cursor": cursor,
                    "direction": "forward",
                    "limit": 1000,
                }),
            )
            .map_err(|error| {
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
                && event.pointer("/params/terminal").and_then(Value::as_str) == Some("completed")
                && let Some(turn_id) = event.pointer("/params/turnId").and_then(Value::as_str)
            {
                ids.push(turn_id.to_owned());
            }
        }
        match result.get("nextCursor").and_then(Value::as_str) {
            Some(next) => cursor = json!(next),
            None => break,
        }
    }
    Ok(ids)
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
