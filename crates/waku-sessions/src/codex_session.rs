//! Codex CLI session discovery, transcript import, and detached forks.
//!
//! The app-server owns Codex's state database and rollout migration rules, so
//! use its public thread APIs instead of reimplementing `codex resume` by
//! walking private SQLite tables or guessing rollout filenames. These probes
//! are bounded, short-lived, and called only from the daemon's background RPC
//! path — never from a render frame.

use std::io::{BufRead as _, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use anyhow::{Context as _, anyhow, bail};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::model::{
    AgentTurn, Message, MessageRole, ProviderResumeCursor, ProviderSessionHistory,
    ProviderSessionSummary, TurnInterruption, TurnStatus,
};

const RPC_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug)]
struct SessionMethodNotFound(String);

impl std::fmt::Display for SessionMethodNotFound {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Codex rejected the session request: {}", self.0)
    }
}

impl std::error::Error for SessionMethodNotFound {}

fn write_json_line(writer: &mut impl Write, value: &Value) -> std::io::Result<()> {
    serde_json::to_writer(&mut *writer, value)?;
    writer.write_all(b"\n")?;
    writer.flush()
}

fn app_server_request(binary: &Path, request: Value) -> anyhow::Result<Value> {
    let cwd = crate::acp_session::catalog_working_directory()?;
    app_server_request_in(binary, &cwd, request)
}

fn app_server_request_in(binary: &Path, cwd: &Path, request: Value) -> anyhow::Result<Value> {
    with_app_server_in(binary, cwd, None, |rpc| rpc(request))
}

fn with_app_server_in<T>(
    binary: &Path,
    cwd: &Path,
    cancelled: Option<&dyn Fn() -> bool>,
    operation: impl FnOnce(&mut dyn FnMut(Value) -> anyhow::Result<Value>) -> anyhow::Result<T>,
) -> anyhow::Result<T> {
    let check_cancelled = || -> anyhow::Result<()> {
        anyhow::ensure!(
            !cancelled.is_some_and(|cancelled| cancelled()),
            "rewind stopped before selecting new provider history"
        );
        Ok(())
    };
    check_cancelled()?;
    let mut command = crate::command_env::command(binary);
    let command = command
        .args(["app-server", "--stdio"])
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = crate::sandbox::spawn(command.command_ref(), None)
        .with_context(|| format!("could not start {} app-server", binary.display()))?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| anyhow!("Codex app-server stdin is unavailable"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow!("Codex app-server stdout is unavailable"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| anyhow!("Codex app-server stderr is unavailable"))?;
    let stderr_reader = std::thread::spawn(move || {
        let _ = std::io::copy(&mut std::io::BufReader::new(stderr), &mut std::io::sink());
    });
    let (tx, rx) = mpsc::channel();
    let reader = std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if let Ok(value) = serde_json::from_str::<Value>(&line)
                && tx.send(value).is_err()
            {
                break;
            }
        }
    });

    let deadline = Instant::now() + RPC_TIMEOUT;
    let receive_response = |id| -> anyhow::Result<Value> {
        loop {
            check_cancelled()?;
            let remaining = deadline
                .checked_duration_since(Instant::now())
                .ok_or_else(|| anyhow!("Codex app-server session request timed out"))?;
            let wait = if cancelled.is_some() {
                remaining.min(Duration::from_millis(10))
            } else {
                remaining
            };
            let value = match rx.recv_timeout(wait) {
                Ok(value) => value,
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    bail!("Codex app-server disconnected before its session reply");
                }
            };
            if value.get("method").is_none() && value.get("id").and_then(Value::as_u64) == Some(id)
            {
                if let Some(error) = value.pointer("/error/message").and_then(Value::as_str) {
                    if value.pointer("/error/code").and_then(Value::as_i64) == Some(-32601) {
                        return Err(SessionMethodNotFound(error.to_owned()).into());
                    }
                    bail!("Codex rejected the session request: {error}");
                }
                return Ok(value);
            }
        }
    };
    let response = (|| -> anyhow::Result<T> {
        write_json_line(
            &mut stdin,
            &json!({
                "method": "initialize",
                "id": 0,
                "params": {
                    "clientInfo": {
                        "name": "waku",
                        "title": "Goddard",
                        "version": env!("CARGO_PKG_VERSION")
                    },
                    "capabilities": { "experimentalApi": true }
                }
            }),
        )?;
        receive_response(0)?;
        write_json_line(&mut stdin, &json!({"method": "initialized", "params": {}}))?;
        let mut next_id = 0_u64;
        operation(&mut |mut request| {
            check_cancelled()?;
            next_id += 1;
            request["id"] = json!(next_id);
            write_json_line(&mut stdin, &request)?;
            receive_response(next_id)
        })
    })();

    // A fork response still leaves a live writer in this app-server. Let EOF
    // flush and close it before handing the new thread to another process.
    // Also wait for stdout to close: the configured CLI may wrap Codex in Node.
    drop(stdin);
    // A cancelled fork may leave an unreferenced copy, but never changes the
    // original history. Retire this owned process instead of waiting for EOF.
    if cancelled.is_some_and(|cancelled| cancelled()) {
        let _ = child.kill();
    }
    let shutdown_deadline = Instant::now() + RPC_TIMEOUT;
    let shutdown = (|| -> anyhow::Result<()> {
        loop {
            check_cancelled()?;
            if let Some(status) = child.try_wait()? {
                if !status.success() {
                    bail!("Codex app-server exited with {status}");
                }
                if reader.is_finished() {
                    return Ok(());
                }
            }
            if Instant::now() >= shutdown_deadline {
                bail!("Codex app-server did not finish closing its session");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    })();
    if shutdown.is_err() {
        let _ = child.kill();
        let _ = child.wait();
    }
    let _ = reader.join();
    let _ = stderr_reader.join();
    let response = response?;
    shutdown?;
    Ok(response)
}

/// Create the fork outside the source session's resident app-server. Returning
/// only after this process exits makes the cursor immediately resumable.
pub fn fork_session_at_turn(
    binary: &Path,
    cwd: &Path,
    thread_id: &str,
    last_turn_id: &str,
) -> anyhow::Result<ProviderResumeCursor> {
    let response = app_server_request_in(binary, cwd, fork_request(cwd, thread_id, last_turn_id))?;
    fork_cursor(&response)
}

/// Fork while omitting recent turns, leaving the source history intact.
/// Page reads and the fork share one request deadline and app-server process.
pub fn fork_session_without_recent_turns(
    binary: &Path,
    cwd: &Path,
    thread_id: &str,
    turns_to_remove: usize,
    cancelled: Option<&dyn Fn() -> bool>,
) -> anyhow::Result<ProviderResumeCursor> {
    let turns_to_read = turns_to_remove
        .checked_add(1)
        .ok_or_else(|| anyhow!("too many Codex turns requested"))?;
    let response = with_app_server_in(binary, cwd, cancelled, |rpc| {
        let mut remaining = turns_to_read;
        let mut cursor: Option<String> = None;
        let mut seen_cursors = std::collections::HashSet::new();
        loop {
            let response = rpc(json!({
                "method": "thread/turns/list",
                "params": {
                    "threadId": thread_id,
                    "sortDirection": "desc",
                    "itemsView": "notLoaded",
                    "limit": remaining.min(100),
                    "cursor": cursor,
                }
            }));
            let response = match response {
                Ok(response) => response,
                Err(error) if error.is::<SessionMethodNotFound>() => {
                    // Older app-servers expose history through thread/read.
                    // Only an absent method permits this compatibility path.
                    let response = rpc(json!({
                        "method": "thread/read",
                        "params": {"threadId": thread_id, "includeTurns": true}
                    }))?;
                    let turns = response
                        .pointer("/result/thread/turns")
                        .and_then(Value::as_array)
                        .ok_or_else(|| anyhow!("Codex returned invalid thread history"))?;
                    let id = turns
                        .len()
                        .checked_sub(turns_to_read)
                        .and_then(|index| turns.get(index))
                        .and_then(|turn| turn.get("id"))
                        .and_then(Value::as_str)
                        .filter(|id| !id.is_empty())
                        .ok_or_else(|| anyhow!("Codex has no completed turn at that response"))?;
                    return rpc(fork_request(cwd, thread_id, id));
                }
                Err(error) => return Err(error),
            };
            let page = response
                .pointer("/result/data")
                .and_then(Value::as_array)
                .ok_or_else(|| anyhow!("Codex returned an invalid turn page"))?;
            if let Some(turn) = page.get(remaining - 1) {
                let id = turn
                    .get("id")
                    .and_then(Value::as_str)
                    .filter(|id| !id.is_empty())
                    .ok_or_else(|| anyhow!("Codex returned a turn without an ID"))?;
                return rpc(fork_request(cwd, thread_id, id));
            }
            let next = response
                .pointer("/result/nextCursor")
                .and_then(Value::as_str)
                .filter(|next| !next.is_empty() && !page.is_empty())
                .ok_or_else(|| anyhow!("Codex has fewer turns than the requested rewind"))?;
            anyhow::ensure!(
                seen_cursors.insert(next.to_owned()),
                "Codex repeated a turn-page cursor"
            );
            remaining -= page.len();
            cursor = Some(next.to_owned());
        }
    })?;
    fork_cursor(&response)
}

fn fork_request(cwd: &Path, thread_id: &str, last_turn_id: &str) -> Value {
    json!({
        "method": "thread/fork",
        "params": {
            "threadId": thread_id,
            "lastTurnId": last_turn_id,
            "cwd": cwd,
            "excludeTurns": true,
            "deferGoalContinuation": true
        }
    })
}

fn fork_cursor(response: &Value) -> anyhow::Result<ProviderResumeCursor> {
    let thread_id = response
        .pointer("/result/thread/id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .ok_or_else(|| anyhow!("Codex returned no forked thread ID."))?;
    Ok(ProviderResumeCursor::Codex {
        thread_id: thread_id.to_owned(),
    })
}

fn title_from_prompt(prompt: &str) -> Option<String> {
    let mut title = prompt
        .split_whitespace()
        .take(7)
        .collect::<Vec<_>>()
        .join(" ");
    if title.is_empty() {
        return None;
    }
    if title.chars().count() > 54 {
        title = format!("{}…", title.chars().take(53).collect::<String>());
    }
    Some(title)
}

fn parse_session_summaries(response: &Value, limit: usize) -> Vec<ProviderSessionSummary> {
    response
        .pointer("/result/data")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|thread| {
            thread.get("ephemeral").and_then(Value::as_bool) != Some(true)
                && thread.get("parentThreadId").is_none_or(Value::is_null)
        })
        .filter_map(|thread| {
            let id = thread.get("id").and_then(Value::as_str)?.trim();
            if id.is_empty() {
                return None;
            }
            let cwd = PathBuf::from(thread.get("cwd").and_then(Value::as_str)?);
            if !cwd.is_absolute() || !cwd.is_dir() {
                return None;
            }
            let title = thread
                .get("name")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|title| !title.is_empty())
                .map(str::to_owned)
                .or_else(|| {
                    thread
                        .get("preview")
                        .and_then(Value::as_str)
                        .and_then(title_from_prompt)
                })?;
            let created_at = thread
                .get("createdAt")
                .and_then(Value::as_u64)
                .unwrap_or_default();
            let updated_at = thread
                .get("recencyAt")
                .and_then(Value::as_u64)
                .or_else(|| thread.get("updatedAt").and_then(Value::as_u64))
                .unwrap_or(created_at)
                .max(created_at);
            Some(ProviderSessionSummary {
                cursor: ProviderResumeCursor::Codex {
                    thread_id: id.to_owned(),
                },
                title,
                cwd,
                cwd_missing: false,
                created_at,
                updated_at,
            })
        })
        .take(limit)
        .collect()
}

/// Ask Codex for CLI-owned threads, excluding exec, editor, app-server and
/// sub-agent rollouts. Goddard-created app-server threads are filtered again by
/// native cursor in the daemon catalog.
pub fn list_provider_sessions(
    binary: &Path,
    limit: usize,
) -> anyhow::Result<Vec<ProviderSessionSummary>> {
    if limit == 0 {
        return Ok(Vec::new());
    }
    let response = app_server_request(
        binary,
        json!({
            "method": "thread/list",
            "id": 1,
            "params": {
                "limit": limit,
                "sortKey": "updated_at",
                "sortDirection": "desc",
                "sourceKinds": ["cli"]
            }
        }),
    )?;
    Ok(parse_session_summaries(&response, limit))
}

fn codex_user_text(item: &Value) -> Option<String> {
    let text = item
        .get("content")
        .and_then(Value::as_array)?
        .iter()
        .filter(|content| content.get("type").and_then(Value::as_str) == Some("text"))
        .filter_map(|content| content.get("text").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("\n\n");
    (!text.trim().is_empty()).then_some(text)
}

fn parse_session_history(response: &Value) -> ProviderSessionHistory {
    let native_turns = response
        .pointer("/result/thread/turns")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut history = ProviderSessionHistory::default();

    for native_turn in native_turns {
        let turn_id = native_turn
            .get("id")
            .and_then(Value::as_str)
            .and_then(|id| Uuid::parse_str(id).ok())
            .unwrap_or_else(Uuid::new_v4);
        let started_at = native_turn
            .get("startedAt")
            .and_then(Value::as_u64)
            .unwrap_or_default();
        let completed_at =
            if native_turn.get("status").and_then(Value::as_str) == Some("inProgress") {
                None
            } else {
                native_turn
                    .get("completedAt")
                    .and_then(Value::as_u64)
                    .or(Some(started_at))
            };
        let status = match native_turn.get("status").and_then(Value::as_str) {
            Some("failed") => TurnStatus::Failed,
            Some("interrupted" | "inProgress") => TurnStatus::Interrupted,
            _ => TurnStatus::Completed,
        };
        history.turns.push(AgentTurn {
            id: turn_id,
            turn_count: history.turns.len() + 1,
            status,
            provider_turn_started: true,
            provider_started_at: None,
            provider_resume_at: None,
            interruption: (status == TurnStatus::Interrupted).then_some(TurnInterruption::Provider),
            started_at,
            completed_at,
            checkpoint: None,
        });

        for item in native_turn
            .get("items")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let (role, content, created_at) = match item.get("type").and_then(Value::as_str) {
                Some("userMessage") => {
                    let Some(content) = codex_user_text(item) else {
                        continue;
                    };
                    (MessageRole::User, content, started_at)
                }
                Some("agentMessage") => {
                    let Some(content) = item
                        .get("text")
                        .and_then(Value::as_str)
                        .filter(|text| !text.trim().is_empty())
                    else {
                        continue;
                    };
                    (
                        MessageRole::Assistant,
                        content.to_owned(),
                        completed_at.unwrap_or(started_at),
                    )
                }
                _ => continue,
            };
            let mut message = Message::new_for_turn(role, content, turn_id);
            message.created_at = created_at;
            history.messages.push(message);
        }
    }
    history
}

fn retain_recent_messages(history: &mut ProviderSessionHistory, visible_turn_limit: usize) {
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

/// Load the complete native turn sequence so Goddard's provider turn counts stay
/// aligned for later fork and rollback operations. Only recent display text is
/// retained; Codex remains authoritative for the full conversation context.
pub fn provider_session_history(
    binary: &Path,
    thread_id: &str,
    visible_turn_limit: usize,
) -> anyhow::Result<ProviderSessionHistory> {
    if thread_id.trim().is_empty() || visible_turn_limit == 0 {
        return Ok(ProviderSessionHistory::default());
    }
    let response = app_server_request(
        binary,
        json!({
            "method": "thread/read",
            "id": 1,
            "params": {
                "threadId": thread_id,
                "includeTurns": true
            }
        }),
    )?;
    let mut history = parse_session_history(&response);
    retain_recent_messages(&mut history, visible_turn_limit);
    Ok(history)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn stopping_a_native_fork_reaps_its_owned_peer_without_selecting_history() {
        use std::os::unix::fs::PermissionsExt as _;
        use std::sync::atomic::{AtomicBool, Ordering};
        for phase in ["read", "fork", "close"] {
            let root = std::env::temp_dir().join(format!("codex-stop-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir(&root).unwrap();
            let binary = root.join("peer");
            std::fs::write(&binary, include_str!("fixtures/codex_fork_stop.py")).unwrap();
            std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();
            std::fs::write(root.join("phase"), phase).unwrap();
            let stopped = AtomicBool::new(false);
            std::thread::scope(|scope| {
                let worker = scope.spawn(|| {
                    fork_session_without_recent_turns(
                        &binary,
                        &root,
                        "original",
                        1,
                        Some(&|| stopped.load(Ordering::Acquire)),
                    )
                });
                let deadline = Instant::now() + Duration::from_secs(5);
                while !root.join("held").exists() && Instant::now() < deadline {
                    std::thread::sleep(Duration::from_millis(2));
                }
                let reached = root.join("held").exists();
                let began = Instant::now();
                stopped.store(true, Ordering::Release);
                let result = worker.join().unwrap();
                assert!(reached, "peer did not reach {phase}");
                assert!(result.is_err(), "Stop selected a native copy in {phase}");
                assert!(
                    began.elapsed() < Duration::from_secs(1),
                    "Stop waited in {phase}"
                );
            });
            let pid: i32 = std::fs::read_to_string(root.join("pid"))
                .unwrap()
                .parse()
                .unwrap();
            // SAFETY: signal zero only probes our recorded owned fixture pid.
            assert_eq!(unsafe { libc::kill(pid, 0) }, -1, "peer survived {phase}");
            assert_eq!(
                std::fs::read_to_string(root.join("original")).unwrap(),
                "unchanged"
            );
            std::fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    fn unfinished_native_turn_is_not_treated_as_a_completed_history_snapshot() {
        let history = parse_session_history(&json!({"result": {"thread": {"turns": [{
            "id": Uuid::new_v4(), "status": "inProgress", "startedAt": 10,
            "items": [{"type": "userMessage", "content": [{"type": "text", "text": "still running"}]}]
        }]}}}));
        assert!(history.turns[0].completed_at.is_none());
    }

    #[test]
    fn parses_thread_catalog_metadata_and_filters_subagents() {
        let root = std::env::temp_dir();
        let response = json!({"result": {"data": [
            {
                "id": "01900000-0000-7000-8000-000000000001",
                "name": "Resume this thread",
                "preview": "ignored fallback",
                "cwd": root,
                "createdAt": 10,
                "updatedAt": 20,
                "recencyAt": 25,
                "ephemeral": false,
                "parentThreadId": null
            },
            {
                "id": "01900000-0000-7000-8000-000000000002",
                "preview": "sub-agent",
                "cwd": root,
                "createdAt": 10,
                "updatedAt": 20,
                "ephemeral": false,
                "parentThreadId": "01900000-0000-7000-8000-000000000001"
            }
        ]}});

        let sessions = parse_session_summaries(&response, 10);
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].title, "Resume this thread");
        assert_eq!(sessions[0].updated_at, 25);
    }

    #[test]
    fn imports_recent_turn_text_in_chronological_order() {
        let response = json!({"result": {"thread": {"turns": [
            {
                "id": "01900000-0000-7000-8000-000000000001",
                "status": "completed",
                "startedAt": 10,
                "completedAt": 20,
                "items": [
                    {"type":"userMessage","id":"u1","content":[{"type":"text","text":"first"}]},
                    {"type":"agentMessage","id":"a1","text":"done"}
                ]
            },
            {
                "id": "01900000-0000-7000-8000-000000000002",
                "status": "completed",
                "startedAt": 30,
                "completedAt": 40,
                "items": [
                    {"type":"userMessage","id":"u2","content":[{"type":"text","text":"second"}]},
                    {"type":"agentMessage","id":"a2","text":"later"}
                ]
            }
        ]}}});

        let mut history = parse_session_history(&response);
        retain_recent_messages(&mut history, 1);
        assert_eq!(history.turns.len(), 2);
        assert_eq!(history.messages.len(), 2);
        assert_eq!(history.messages[0].content, "second");
        assert_eq!(history.messages[1].content, "later");
        assert_eq!(history.messages[0].turn_id, Some(history.turns[1].id));
        assert_eq!(history.turns[1].turn_count, 2);
    }
}
