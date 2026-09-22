//! Cursor's Cloud Agents API (v1): a durable `bc-…` agent plus one run per
//! prompt. Each run exposes an SSE stream of tool calls, so the transcript
//! is real — not a status poll.
//!
//! Credential: `CURSOR_API_KEY` from Cursor Dashboard → API Keys, sent as
//! HTTP Basic `key:` like the docs' `-u YOUR_API_KEY:` examples.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use anyhow::Context as _;
use serde_json::{Value, json};

use super::{
    BackendEvent, BackendSink, CloudBackend, CloudLaunch, CloudTarget, LaunchContext, SendOutcome,
    env_var, http,
};
use crate::model::ProviderResumeCursor;

const API: &str = "https://api.cursor.com/v1";
const REPOLL: Duration = Duration::from_secs(10);

pub(crate) struct CursorCloud {
    api_key: Option<String>,
}

impl CursorCloud {
    pub fn from_env() -> Self {
        Self {
            api_key: std::env::var("CURSOR_API_KEY")
                .ok()
                .filter(|key| !key.is_empty()),
        }
    }

    fn headers(&self) -> anyhow::Result<Vec<(String, String)>> {
        let key = env_var("CURSOR_API_KEY").or_else(|_| {
            self.api_key
                .clone()
                .context("CURSOR_API_KEY is not set — create one in Cursor Dashboard → API Keys")
        })?;
        use base64::Engine as _;
        Ok(vec![
            (
                "Authorization".into(),
                format!(
                    "Basic {}",
                    base64::engine::general_purpose::STANDARD.encode(format!("{key}:"))
                ),
            ),
            ("Content-Type".into(), "application/json".into()),
            ("Accept".into(), "text/event-stream".into()),
        ])
    }
}

/// One frame of a run's SSE stream: `data: {…}` already parsed.
enum RunFrame {
    /// `{runId, status}` heartbeat — the status word itself is not needed.
    Status,
    /// A tool call entering or leaving `running`.
    ToolCall { name: String, running: bool },
    /// Terminal: FINISHED/ERROR/CANCELLED/EXPIRED with the run's tail.
    Terminal {
        status: String,
        text: Option<String>,
    },
}

fn parse_frame(payload: &str) -> Option<RunFrame> {
    let value: Value = serde_json::from_str(payload).ok()?;
    if let Some(name) = value.get("name").and_then(Value::as_str) {
        return Some(RunFrame::ToolCall {
            name: name.to_owned(),
            running: value.get("status").and_then(Value::as_str) == Some("running"),
        });
    }
    let status = value.get("status").and_then(Value::as_str)?.to_owned();
    if matches!(
        status.as_str(),
        "FINISHED" | "ERROR" | "CANCELLED" | "EXPIRED"
    ) {
        return Some(RunFrame::Terminal {
            status,
            text: value.get("text").and_then(Value::as_str).map(str::to_owned),
        });
    }
    Some(RunFrame::Status)
}

impl CloudBackend for CursorCloud {
    fn launch(
        &self,
        _ctx: &LaunchContext,
        target: &CloudTarget,
        prompt: &str,
    ) -> anyhow::Result<CloudLaunch> {
        let response = http::request(
            "POST",
            &format!("{API}/agents"),
            &self.headers()?,
            Some(&json!({
                "prompt": { "text": prompt },
                "repos": [{ "url": target.https_url, "startingRef": target.base_branch }],
                "autoCreatePR": true,
            })),
        )?;
        let agent_id = response
            .body
            .pointer("/agent/id")
            .and_then(Value::as_str)
            .context("Cursor accepted the task but returned no agent id")?
            .to_owned();
        let run_id = response
            .body
            .pointer("/run/id")
            .and_then(Value::as_str)
            .map(str::to_owned);
        Ok(CloudLaunch {
            cursor: ProviderResumeCursor::Cursor {
                session_id: agent_id.clone(),
                fork_context: None,
            },
            url: response
                .body
                .pointer("/agent/url")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .or_else(|| Some(format!("https://cursor.com/agents/{agent_id}"))),
            detail: None,
            extra: json!({ "agent_id": agent_id, "run_id": run_id }),
            process: None,
        })
    }

    fn watch(&self, launch: &CloudLaunch, sink: BackendSink, stop: Arc<AtomicBool>) {
        let (Some(agent_id), Some(run_id)) = (
            launch.extra.get("agent_id").and_then(Value::as_str),
            launch.extra.get("run_id").and_then(Value::as_str),
        ) else {
            return;
        };
        let Ok(headers) = self.headers() else { return };
        let url = format!("{API}/agents/{agent_id}/runs/{run_id}/stream");
        match http::stream("GET", &url, &headers) {
            Ok(mut stream) => {
                while !stop.load(Ordering::Relaxed) {
                    let Some(payload) = stream.next_event() else {
                        break;
                    };
                    match parse_frame(&payload) {
                        Some(RunFrame::ToolCall { name, running }) if running => {
                            let _ = sink.send(BackendEvent::Progress(name));
                        }
                        Some(RunFrame::Terminal { status, text }) => {
                            let _ = sink.send(BackendEvent::Finished {
                                success: status == "FINISHED",
                                summary: match status.as_str() {
                                    "FINISHED" => {
                                        text.unwrap_or_else(|| "Cursor finished the run".to_owned())
                                    }
                                    "CANCELLED" => "the Cursor run was cancelled".to_owned(),
                                    other => format!("the Cursor run ended {other}"),
                                },
                            });
                            return;
                        }
                        // Heartbeats and tool-call completions carry
                        // nothing new to show.
                        Some(RunFrame::Status) | Some(RunFrame::ToolCall { .. }) | None => {}
                    }
                }
                return;
            }
            // The stream refused — fall back to polling the run.
            Err(_) => loop {
                if stop.load(Ordering::Relaxed) {
                    return;
                }
                match http::request(
                    "GET",
                    &format!("{API}/agents/{agent_id}/runs/{run_id}"),
                    &headers,
                    None,
                ) {
                    Ok(response) => {
                        let status = response
                            .body
                            .pointer("/run/status")
                            .or_else(|| response.body.get("status"))
                            .and_then(Value::as_str)
                            .unwrap_or_default();
                        match status {
                            "FINISHED" | "ERROR" | "CANCELLED" | "EXPIRED" => {
                                let _ = sink.send(BackendEvent::Finished {
                                    success: status == "FINISHED",
                                    summary: format!("the Cursor run ended {status}"),
                                });
                                return;
                            }
                            _ => {
                                let _ = sink.send(BackendEvent::Running { detail: None });
                            }
                        }
                    }
                    Err(error) => {
                        let _ = sink.send(BackendEvent::Finished {
                            success: false,
                            summary: format!("lost contact with the Cursor run — {error:#}"),
                        });
                        return;
                    }
                }
                thread::sleep(REPOLL);
            },
        }
    }

    fn supports_messages(&self) -> bool {
        true
    }

    fn send_message(&self, launch: &CloudLaunch, text: &str) -> anyhow::Result<SendOutcome> {
        let agent_id = launch
            .extra
            .get("agent_id")
            .and_then(Value::as_str)
            .context("Cursor launch is missing its agent id")?;
        let response = http::request(
            "POST",
            &format!("{API}/agents/{agent_id}/runs"),
            &self.headers()?,
            Some(&json!({ "prompt": { "text": text } })),
        )?;
        let run_id = response
            .body
            .pointer("/run/id")
            .or_else(|| response.body.get("id"))
            .and_then(Value::as_str)
            .map(str::to_owned);
        let mut relaunch = launch.clone();
        relaunch.extra["run_id"] = json!(run_id);
        Ok(SendOutcome::Respawned(relaunch))
    }

    fn cancel(&self, launch: &CloudLaunch) -> anyhow::Result<()> {
        let (Some(agent_id), Some(run_id)) = (
            launch.extra.get("agent_id").and_then(Value::as_str),
            launch.extra.get("run_id").and_then(Value::as_str),
        ) else {
            return Ok(());
        };
        http::request(
            "POST",
            &format!("{API}/agents/{agent_id}/runs/{run_id}/cancel"),
            &self.headers()?,
            None,
        )?;
        Ok(())
    }

    fn launch_from_cursor(&self, cursor: &ProviderResumeCursor) -> Option<CloudLaunch> {
        let ProviderResumeCursor::Cursor { session_id, .. } = cursor else {
            return None;
        };
        if session_id.is_empty() {
            return None;
        }
        // The run id is not in the cursor — recover the agent's latest run.
        let run_id = self
            .headers()
            .ok()
            .and_then(|headers| {
                http::request(
                    "GET",
                    &format!("{API}/agents/{session_id}/runs"),
                    &headers,
                    None,
                )
                .ok()
            })
            .and_then(|response| {
                response
                    .body
                    .get("runs")
                    .and_then(Value::as_array)
                    .and_then(|runs| runs.first().cloned())
            })
            .and_then(|run| run.get("id").and_then(Value::as_str).map(str::to_owned));
        Some(CloudLaunch {
            cursor: cursor.clone(),
            url: Some(format!("https://cursor.com/agents/{session_id}")),
            detail: None,
            extra: json!({ "agent_id": session_id, "run_id": run_id }),
            process: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_classify() {
        assert!(matches!(
            parse_frame(r#"{"runId":"run-1","status":"RUNNING"}"#),
            Some(RunFrame::Status)
        ));
        assert!(matches!(
            parse_frame(r#"{"callId":"c1","name":"read_file","status":"running"}"#),
            Some(RunFrame::ToolCall { running: true, .. })
        ));
        assert!(matches!(
            parse_frame(r#"{"runId":"run-1","status":"FINISHED","text":"done"}"#),
            Some(RunFrame::Terminal { .. })
        ));
        assert!(matches!(parse_frame("not json"), None));
    }
}
