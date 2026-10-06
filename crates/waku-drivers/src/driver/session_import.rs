//! ACP catalog and history probing through provider drivers.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::schema::v1::{
    ClientCapabilities, Implementation, InitializeRequest, ListSessionsRequest, LoadSessionRequest,
    SessionNotification,
};
use agent_client_protocol::{Agent, Client, ConnectionTo};
use anyhow::{Context as _, anyhow};
use parking_lot::Mutex;
use serde_json::Value;

use crate::acp_session::{
    ProviderSessionCatalog, catalog_working_directory, history_from_updates,
    retain_recent_messages, session_title, timestamp,
};
use crate::model::{
    ProviderKind, ProviderResumeCursor, ProviderSessionHistory, ProviderSessionSummary,
};

const MAX_CATALOG_PAGES: usize = 20;
const MAX_CATALOG_WORKSPACES: usize = 100;
const ACP_REQUEST_TIMEOUT: Duration = Duration::from_secs(20);

fn initialize_request() -> InitializeRequest {
    InitializeRequest::new(ProtocolVersion::V1)
        .client_capabilities(ClientCapabilities::new().terminal(false))
        .client_info(Implementation::new("waku", env!("CARGO_PKG_VERSION")))
}

/// Lists every session exposed by an ACP agent. An empty `cwd_filters` list
/// requests the provider's global catalog; providers such as Kimi that require
/// a cwd pass their native workspace index here instead.
pub fn list_provider_sessions(
    provider: ProviderKind,
    binary: &Path,
    cwd_filters: &[PathBuf],
    limit: usize,
) -> anyhow::Result<ProviderSessionCatalog> {
    if limit == 0 {
        return Ok(Vec::new().into());
    }
    let agent = crate::driver::catalog_agent(provider, binary, &catalog_working_directory()?)?;
    let filters = if cwd_filters.is_empty() {
        vec![None]
    } else {
        cwd_filters
            .iter()
            .take(MAX_CATALOG_WORKSPACES)
            .cloned()
            .map(Some)
            .collect()
    };

    let request = Client.builder().name("waku-session-catalog").connect_with(
        agent,
        async move |connection: ConnectionTo<Agent>| {
            let initialize = connection
                .send_request(initialize_request())
                .block_task()
                .await?;
            if initialize
                .agent_capabilities
                .session_capabilities
                .list
                .is_none()
            {
                return Ok(ProviderSessionCatalog::unsupported());
            }

            let mut found = Vec::new();
            let mut seen = HashSet::new();
            for cwd in filters {
                let mut cursor = None;
                let mut seen_cursors = HashSet::new();
                for _ in 0..MAX_CATALOG_PAGES {
                    let mut request = ListSessionsRequest::new().cursor(cursor.clone());
                    if let Some(cwd) = cwd.clone() {
                        request = request.cwd(cwd);
                    }
                    let response = connection.send_request(request).block_task().await?;
                    for session in response.sessions {
                        let session_id = session.session_id.to_string();
                        if session_id.trim().is_empty()
                            || !session.cwd.is_absolute()
                            || !seen.insert(session_id.clone())
                        {
                            continue;
                        }
                        let updated_at = timestamp(session.updated_at.as_deref());
                        // `session/list` carries no created field; providers
                        // that know it publish it under `_meta` (Devin's
                        // `cognition.ai/createdAt`). Missing reads as "as old
                        // as the last activity" rather than as epoch.
                        let created_at = session
                            .meta
                            .as_ref()
                            .and_then(|meta| {
                                ["cognition.ai/createdAt", "createdAt"]
                                    .into_iter()
                                    .find_map(|key| meta.get(key).and_then(Value::as_str))
                            })
                            .map(|value| timestamp(Some(value)))
                            .filter(|at| *at > 0)
                            .unwrap_or(updated_at);
                        found.push(ProviderSessionSummary {
                            cursor: ProviderResumeCursor::from_session_id(
                                provider,
                                session_id.clone(),
                            ),
                            title: session_title(provider, session.title.as_deref(), &session_id),
                            cwd: session.cwd,
                            cwd_missing: false,
                            created_at,
                            updated_at,
                        });
                    }
                    if found.len() >= limit {
                        break;
                    }
                    let Some(next) = response.next_cursor.filter(|next| !next.is_empty()) else {
                        break;
                    };
                    if !seen_cursors.insert(next.clone()) {
                        break;
                    }
                    cursor = Some(next);
                }
                if found.len() >= limit {
                    break;
                }
            }
            found.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
            found.truncate(limit);
            Ok(found.into())
        },
    );
    smol::block_on(smol::future::race(
        async move { request.await.map_err(anyhow::Error::new) },
        async move {
            smol::Timer::after(ACP_REQUEST_TIMEOUT).await;
            Err(anyhow!(
                "{} ACP session catalog timed out",
                provider.display_name()
            ))
        },
    ))
    .with_context(|| format!("{} could not list ACP sessions", provider.display_name()))
}

/// Replays the provider's user-visible transcript through ACP `session/load`.
///
/// `fallback_timestamp` dates turns and messages the provider's replay leaves
/// unstamped — the catalog's last-activity stamp when the client knows it.
/// Without either source the import time applies, so no row ever shows the
/// epoch.
pub fn provider_session_history(
    provider: ProviderKind,
    binary: &Path,
    cwd: &Path,
    session_id: &str,
    visible_turn_limit: usize,
    fallback_timestamp: Option<u64>,
) -> anyhow::Result<ProviderSessionHistory> {
    if session_id.trim().is_empty() || visible_turn_limit == 0 {
        return Ok(ProviderSessionHistory::default());
    }
    let agent = crate::driver::catalog_agent(provider, binary, cwd)?;
    let updates = Arc::new(Mutex::new(Vec::<Value>::new()));
    let captured = Arc::clone(&updates);
    let cwd = cwd.to_path_buf();
    let session_id = session_id.to_owned();

    let request = Client
        .builder()
        .name("waku-session-import")
        .on_receive_notification(
            async move |notification: SessionNotification, _connection| {
                if let Ok(update) = serde_json::to_value(notification.update) {
                    captured.lock().push(update);
                }
                Ok(())
            },
            agent_client_protocol::on_receive_notification!(),
        )
        .connect_with(agent, async move |connection: ConnectionTo<Agent>| {
            let initialize = connection
                .send_request(initialize_request())
                .block_task()
                .await?;
            if !initialize.agent_capabilities.load_session {
                return Err(agent_client_protocol::Error::new(
                    agent_client_protocol::ErrorCode::MethodNotFound.into(),
                    "the agent does not support session/load",
                ));
            }
            connection
                .send_request(LoadSessionRequest::new(session_id, cwd))
                .block_task()
                .await?;
            // ACP ordering places replay before the load response. A short
            // grace still covers providers that flush their final queued
            // notification immediately after that response.
            smol::Timer::after(Duration::from_millis(50)).await;
            Ok(())
        });
    smol::block_on(smol::future::race(
        async move { request.await.map_err(anyhow::Error::new) },
        async move {
            smol::Timer::after(ACP_REQUEST_TIMEOUT).await;
            Err(anyhow!(
                "{} ACP session replay timed out",
                provider.display_name()
            ))
        },
    ))
    .with_context(|| format!("{} could not replay the session", provider.display_name()))?;

    let updates = std::mem::take(&mut *updates.lock());
    let mut history = history_from_updates(provider, &updates, fallback_timestamp);
    retain_recent_messages(&mut history, visible_turn_limit);
    Ok(history)
}
