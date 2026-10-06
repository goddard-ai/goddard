//! Devin's hosted sessions API — the fullest cloud surface: a durable
//! session with message history, follow-ups, and termination.
//!
//! Credentials come from the environment like every other provider secret:
//! `DEVIN_API_KEY` plus `DEVIN_ORG_ID` for `cog_` service keys (the v3 API
//! is organization-scoped). `apk_` personal keys ride the v1 API, which has
//! no message listing — those sessions report status only.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use anyhow::Context as _;
use serde_json::{Value, json};

use super::{
    BackendEvent, BackendSink, CloudBackend, CloudLaunch, CloudTarget, LaunchContext, SendOutcome,
    http, prompt_title,
};
use crate::model::ProviderResumeCursor;

const POLL_INTERVAL: Duration = Duration::from_secs(8);

pub(crate) struct DevinCloud {
    api_url: String,
    api_key: Option<String>,
    org_id: Option<String>,
}

impl DevinCloud {
    pub fn from_env() -> Self {
        Self {
            api_url: std::env::var("DEVIN_API_URL")
                .ok()
                .filter(|url| !url.is_empty())
                .unwrap_or_else(|| "https://api.devin.ai".to_owned()),
            api_key: std::env::var("DEVIN_API_KEY")
                .ok()
                .filter(|key| !key.is_empty()),
            org_id: std::env::var("DEVIN_ORG_ID")
                .ok()
                .filter(|org| !org.is_empty()),
        }
    }

    /// The API root for this key. `cog_` service keys are v3 and
    /// organization-scoped; personal keys use v1.
    fn base(&self) -> anyhow::Result<String> {
        let key = self.key()?;
        if key.starts_with("cog_") {
            let org = self.org_id.clone().context(
                "DEVIN_ORG_ID is not set — service keys (cog_) need it; \
                 find it at app.devin.ai → Settings → Devin API",
            )?;
            Ok(format!("{}/v3/organizations/{org}", self.api_url))
        } else {
            Ok(format!("{}/v1", self.api_url))
        }
    }

    fn key(&self) -> anyhow::Result<&str> {
        self.api_key.as_deref().context(
            "DEVIN_API_KEY is not set — create a key at app.devin.ai → Settings → Devin API",
        )
    }

    fn headers(&self) -> anyhow::Result<Vec<(String, String)>> {
        Ok(vec![
            ("Authorization".into(), format!("Bearer {}", self.key()?)),
            ("Content-Type".into(), "application/json".into()),
        ])
    }

    /// v3 exposes the message feed; v1 does not.
    fn messages_url(&self, session_id: &str) -> anyhow::Result<Option<String>> {
        if self.key()?.starts_with("cog_") {
            Ok(Some(format!(
                "{}/sessions/{session_id}/messages",
                self.base()?
            )))
        } else {
            Ok(None)
        }
    }

    fn send_url(&self, session_id: &str) -> anyhow::Result<String> {
        let base = self.base()?;
        if self.key()?.starts_with("cog_") {
            Ok(format!("{base}/sessions/{session_id}/messages"))
        } else {
            Ok(format!("{base}/sessions/{session_id}/message"))
        }
    }
}

impl CloudBackend for DevinCloud {
    fn launch(
        &self,
        _ctx: &LaunchContext,
        target: &CloudTarget,
        prompt: &str,
    ) -> anyhow::Result<CloudLaunch> {
        let prompt = format!(
            "{prompt}\n\n---\nRepository: {}\nBranch: {}",
            target.https_url, target.base_branch
        );
        let mut body = json!({
            "prompt": prompt,
            "title": prompt_title(&prompt),
            "tags": ["goddard"],
        });
        if let Some(slug) = &target.github_slug {
            body["repos"] = json!([slug]);
        }
        let response = http::request(
            "POST",
            &format!("{}/sessions", self.base()?),
            &self.headers()?,
            Some(&body),
        )?;
        let session_id = response
            .body
            .get("session_id")
            .or_else(|| response.body.get("sessionId"))
            .and_then(Value::as_str)
            .map(str::to_owned)
            .context("Devin accepted the task but returned no session id")?;
        Ok(CloudLaunch {
            cursor: ProviderResumeCursor::Devin {
                session_id: session_id.clone(),
            },
            url: Some(format!("https://app.devin.ai/sessions/{session_id}")),
            detail: None,
            extra: json!({ "session_id": session_id }),
            process: None,
        })
    }

    fn watch(&self, launch: &CloudLaunch, sink: BackendSink, stop: Arc<AtomicBool>) {
        let Some(session_id) = launch
            .extra
            .get("session_id")
            .and_then(Value::as_str)
            .map(str::to_owned)
        else {
            return;
        };
        let Ok(headers) = self.headers() else {
            return;
        };
        let Ok(base) = self.base() else { return };
        let messages_url = self.messages_url(&session_id).ok().flatten();
        let mut message_cursor: Option<String> = None;
        let mut waiting_reported = false;
        while !stop.load(Ordering::Relaxed) {
            // Newest session state first — a session that finished between
            // polls should still flush its last messages before settling.
            if let Some(url) = &messages_url {
                let mut url = url.clone();
                if let Some(cursor) = &message_cursor {
                    url = format!("{url}?after={cursor}&first=100");
                }
                match http::request("GET", &url, &headers, None) {
                    Ok(response) => {
                        for item in response
                            .body
                            .get("items")
                            .and_then(Value::as_array)
                            .into_iter()
                            .flatten()
                        {
                            if item.get("source").and_then(Value::as_str) == Some("devin")
                                && let Some(message) = item.get("message").and_then(Value::as_str)
                            {
                                let _ = sink.send(BackendEvent::Message(message.to_owned()));
                            }
                        }
                        if let Some(cursor) =
                            response.body.get("end_cursor").and_then(Value::as_str)
                        {
                            message_cursor = Some(cursor.to_owned());
                        }
                    }
                    Err(error) => {
                        let _ = sink.send(BackendEvent::Finished {
                            success: false,
                            summary: format!("lost contact with the Devin session — {error:#}"),
                        });
                        return;
                    }
                }
            }
            match http::request(
                "GET",
                &format!("{base}/sessions/{session_id}"),
                &headers,
                None,
            ) {
                Ok(response) => {
                    let status = response
                        .body
                        .get("status")
                        .and_then(Value::as_str)
                        .map(str::to_owned);
                    let detail = response
                        .body
                        .get("status_detail")
                        .and_then(Value::as_str)
                        .map(str::to_owned);
                    let prs = response
                        .body
                        .get("pull_requests")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                        .filter_map(|pr| {
                            pr.get("url")
                                .or_else(|| pr.get("pr_url"))
                                .and_then(Value::as_str)
                        })
                        .map(str::to_owned)
                        .collect::<Vec<_>>();
                    let landed = if prs.is_empty() {
                        launch
                            .url
                            .clone()
                            .unwrap_or_else(|| "the Devin session".to_owned())
                    } else {
                        prs.join(", ")
                    };
                    // The suspended/running states that mean "waiting on
                    // you" — park the turn once per stretch, not per poll.
                    let waiting_reason = match (status.as_deref(), detail.as_deref()) {
                        (Some("suspended"), Some(reason))
                            if matches!(reason, "inactivity" | "user_request") =>
                        {
                            Some(reason.replace('_', " "))
                        }
                        (
                            Some("running"),
                            Some(reason @ ("waiting_for_user" | "waiting_for_approval")),
                        ) => Some(reason.replace('_', " ")),
                        _ => None,
                    };
                    match (status.as_deref(), detail.as_deref()) {
                        (_, Some("finished")) | (Some("exit"), _) => {
                            let _ = sink.send(BackendEvent::Finished {
                                success: true,
                                summary: format!("Devin finished — {landed}"),
                            });
                            return;
                        }
                        (Some("error"), _) | (Some("suspended"), Some("error")) => {
                            let _ = sink.send(BackendEvent::Finished {
                                success: false,
                                summary: format!("the Devin session errored — {landed}"),
                            });
                            return;
                        }
                        _ if waiting_reason.is_some() => {
                            if !waiting_reported {
                                waiting_reported = true;
                                let _ = sink.send(BackendEvent::Waiting {
                                    detail: Some(format!(
                                        "Devin is waiting — {}",
                                        waiting_reason.expect("checked")
                                    )),
                                });
                            }
                        }
                        _ => {
                            waiting_reported = false;
                            let _ = sink.send(BackendEvent::Running { detail: None });
                        }
                    }
                }
                Err(error) => {
                    let _ = sink.send(BackendEvent::Finished {
                        success: false,
                        summary: format!("lost contact with the Devin session — {error:#}"),
                    });
                    return;
                }
            }
            thread::sleep(POLL_INTERVAL);
        }
    }

    fn supports_messages(&self) -> bool {
        true
    }

    fn send_message(&self, launch: &CloudLaunch, text: &str) -> anyhow::Result<SendOutcome> {
        let session_id = launch
            .extra
            .get("session_id")
            .and_then(Value::as_str)
            .context("Devin launch is missing its session id")?;
        http::request(
            "POST",
            &self.send_url(session_id)?,
            &self.headers()?,
            Some(&json!({ "message": text })),
        )?;
        Ok(SendOutcome::Delivered)
    }

    fn cancel(&self, launch: &CloudLaunch) -> anyhow::Result<()> {
        let Some(session_id) = launch.extra.get("session_id").and_then(Value::as_str) else {
            return Ok(());
        };
        // DELETE terminates the session outright.
        http::request(
            "DELETE",
            &format!("{}/sessions/{session_id}", self.base()?),
            &self.headers()?,
            None,
        )?;
        Ok(())
    }

    fn launch_from_cursor(&self, cursor: &ProviderResumeCursor) -> Option<CloudLaunch> {
        let ProviderResumeCursor::Devin { session_id } = cursor else {
            return None;
        };
        if session_id.is_empty() {
            return None;
        }
        Some(CloudLaunch {
            cursor: cursor.clone(),
            url: Some(format!("https://app.devin.ai/sessions/{session_id}")),
            detail: None,
            extra: json!({ "session_id": session_id }),
            process: None,
        })
    }
}
