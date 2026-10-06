//! GitHub's Copilot coding agent via `gh agent-task`: a task runs on
//! GitHub's infrastructure and lands as a pull request on the repo.
//!
//! Auth is `gh`'s own login. The task description goes over stdin
//! (`-F -`) so long prompts never hit argv limits. Follow-ups ride the
//! task's pull request — `@copilot` comments re-engage the agent — which
//! only exists once the task has opened one.

use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use anyhow::{Context as _, bail};
use serde_json::{Value, json};

use super::{
    BackendEvent, BackendSink, CloudBackend, CloudLaunch, CloudTarget, LaunchContext, SendOutcome,
    find_url,
};
use crate::model::ProviderResumeCursor;

const POLL_INTERVAL: Duration = Duration::from_secs(20);

pub(crate) struct CopilotCloud;

impl CopilotCloud {
    fn gh(args: &[&str], input: Option<&str>, cwd: &std::path::Path) -> anyhow::Result<String> {
        let mut command = crate::command_env::search_path_command("gh");
        command
            .args(args)
            .current_dir(cwd)
            .stdin(if input.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.spawn().context("failed to launch gh")?;
        if let Some(stdin) = child.stdin.as_mut() {
            use std::io::Write;
            stdin.write_all(input.unwrap_or_default().as_bytes())?;
        }
        drop(child.stdin.take());
        let output = child.wait_with_output().context("gh failed")?;
        if !output.status.success() {
            bail!(
                "`gh {}` failed: {}",
                args.join(" "),
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }

    /// The session id `gh agent-task create` reports — a UUID in the
    /// output, else the newest task's id.
    fn task_id(output: &str, slug: &str, cwd: &std::path::Path) -> anyhow::Result<String> {
        for token in output.split_whitespace() {
            let token = token.trim_matches(|c: char| "\"'<>()".contains(c));
            if uuid::Uuid::parse_str(token).is_ok() {
                return Ok(token.to_owned());
            }
        }
        let listed = Self::gh(
            &[
                "agent-task",
                "list",
                "--repo",
                slug,
                "--json",
                "id",
                "--limit",
                "1",
            ],
            None,
            cwd,
        )?;
        serde_json::from_str::<Value>(&listed)
            .ok()
            .and_then(|tasks| tasks.as_array().and_then(|t| t.first().cloned()))
            .and_then(|task| task.get("id").and_then(Value::as_str).map(str::to_owned))
            .context("gh agent-task create did not report a session id")
    }
}

impl CloudBackend for CopilotCloud {
    fn launch(
        &self,
        ctx: &LaunchContext,
        target: &CloudTarget,
        prompt: &str,
    ) -> anyhow::Result<CloudLaunch> {
        let slug = target.github_slug.clone().context(
            "Copilot cloud tasks need a GitHub remote — this repository's `origin` is not GitHub",
        )?;
        let output = Self::gh(
            &[
                "agent-task",
                "create",
                "-F",
                "-",
                "--repo",
                &slug,
                "--base",
                &target.base_branch,
            ],
            Some(prompt),
            &ctx.cwd,
        )?;
        let session_id = Self::task_id(&output, &slug, &ctx.cwd)?;
        Ok(CloudLaunch {
            cursor: ProviderResumeCursor::Copilot {
                session_id: session_id.clone(),
            },
            url: find_url(&output, "github.com"),
            detail: Some(format!("session {session_id}")),
            extra: json!({ "session_id": session_id, "slug": slug }),
            process: None,
        })
    }

    fn watch(&self, launch: &CloudLaunch, sink: BackendSink, stop: Arc<AtomicBool>) {
        let (Some(session_id), Some(slug)) = (
            launch.extra.get("session_id").and_then(Value::as_str),
            launch.extra.get("slug").and_then(Value::as_str),
        ) else {
            return;
        };
        let cwd = std::env::temp_dir(); // gh --repo overrides; cwd only needs to exist.
        while !stop.load(Ordering::Relaxed) {
            let view = Self::gh(
                &[
                    "agent-task",
                    "view",
                    session_id,
                    "--repo",
                    slug,
                    "--json",
                    "state,pullRequestUrl,pullRequestState,completedAt",
                ],
                None,
                &cwd,
            );
            match view {
                Ok(text) => {
                    let body: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
                    let state = body
                        .get("state")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_ascii_lowercase();
                    let pr = body
                        .get("pullRequestUrl")
                        .and_then(Value::as_str)
                        .map(str::to_owned);
                    if let Some(pr) = &pr {
                        let _ = sink.send(BackendEvent::Progress(format!("opened {pr}")));
                    }
                    let done = body.get("completedAt").and_then(Value::as_str).is_some()
                        || state.contains("complet")
                        || state.contains("fail")
                        || state.contains("error")
                        || state.contains("cancel");
                    if done {
                        let success = !state.contains("fail")
                            && !state.contains("error")
                            && !state.contains("cancel");
                        let _ = sink.send(BackendEvent::Finished {
                            success,
                            summary: pr
                                .unwrap_or_else(|| format!("Copilot task ended — state {state}")),
                        });
                        return;
                    }
                    let _ = sink.send(BackendEvent::Running { detail: pr });
                }
                Err(error) => {
                    let _ = sink.send(BackendEvent::Finished {
                        success: false,
                        summary: format!("lost contact with the Copilot task — {error:#}"),
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

    /// The task's run ends but its pull request keeps the `@copilot`
    /// follow-up channel open — a finished launch is still reachable.
    fn finished_accepts_messages(&self) -> bool {
        true
    }

    fn send_message(&self, launch: &CloudLaunch, text: &str) -> anyhow::Result<SendOutcome> {
        // The task's only follow-up channel is its pull request: an
        // `@copilot` comment re-engages the agent. Before a PR exists the
        // task can't take one.
        let session_id = launch
            .extra
            .get("session_id")
            .and_then(Value::as_str)
            .context("Copilot launch is missing its session id")?;
        let slug = launch
            .extra
            .get("slug")
            .and_then(Value::as_str)
            .context("Copilot launch is missing its repo slug")?;
        let cwd = std::env::temp_dir();
        let view = Self::gh(
            &[
                "agent-task",
                "view",
                session_id,
                "--repo",
                slug,
                "--json",
                "pullRequestUrl",
            ],
            None,
            &cwd,
        )?;
        let pr = serde_json::from_str::<Value>(&view)
            .ok()
            .and_then(|body| {
                body.get("pullRequestUrl")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            })
            .context("the Copilot task has not opened a pull request yet — nothing to reply to")?;
        Self::gh(
            &["pr", "comment", &pr, "--body", &format!("@copilot {text}")],
            None,
            &cwd,
        )?;
        Ok(SendOutcome::Delivered)
    }

    fn launch_from_cursor(&self, cursor: &ProviderResumeCursor) -> Option<CloudLaunch> {
        let ProviderResumeCursor::Copilot { session_id } = cursor else {
            return None;
        };
        if session_id.is_empty() {
            return None;
        }
        Some(CloudLaunch {
            cursor: cursor.clone(),
            url: None,
            detail: Some(format!("session {session_id}")),
            // The slug is not in the cursor — `watch` needs it, and the
            // driver's target resolution provides it at start time.
            extra: json!({ "session_id": session_id }),
            process: None,
        })
    }
}
