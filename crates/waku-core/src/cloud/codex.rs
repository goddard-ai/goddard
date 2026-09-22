//! Codex Cloud via the `codex cloud` CLI: `exec` submits a task into a
//! cloud environment keyed to the GitHub repo, `status`/`list` poll it, and
//! `diff` describes what it produced. Follow-ups aren't exposed on the CLI,
//! so each Goddard prompt is its own cloud task.
//!
//! Auth is the CLI's own login (`codex login`); the environment id — which
//! `exec` requires — is discovered from ChatGPT's backend API using the
//! token `codex cloud list` refreshes into `~/.codex/auth.json`.

use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use anyhow::{Context as _, bail};
use serde_json::{Value, json};

use super::{
    BackendEvent, BackendSink, CloudBackend, CloudLaunch, CloudTarget, LaunchContext, http,
};
use crate::model::ProviderResumeCursor;

const POLL_INTERVAL: Duration = Duration::from_secs(15);

pub(crate) struct CodexCloud;

impl CodexCloud {
    fn run(ctx: &LaunchContext, args: &[&str], input: Option<&str>) -> anyhow::Result<String> {
        let mut command = crate::command_env::search_path_command(&ctx.binary);
        command
            .args(args)
            .current_dir(&ctx.cwd)
            .stdin(if input.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command
            .spawn()
            .with_context(|| format!("failed to launch {}", ctx.binary.display()))?;
        if let Some(stdin) = child.stdin.as_mut() {
            use std::io::Write;
            stdin.write_all(input.unwrap_or_default().as_bytes())?;
        }
        drop(child.stdin.take());
        let output = child.wait_with_output().context("codex cloud failed")?;
        if !output.status.success() {
            bail!(
                "`codex {}` failed: {}",
                args.join(" "),
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }

    /// The ChatGPT access token `codex login` stores — refreshed by running
    /// a cheap `codex cloud` call first, since the CLI manages its own
    /// token lifecycle.
    fn auth_token(ctx: &LaunchContext) -> anyhow::Result<(String, Option<String>)> {
        let _ = Self::run(ctx, &["cloud", "list", "--json", "--limit", "1"], None);
        let auth_path = dirs::home_dir()
            .map(|home| home.join(".codex").join("auth.json"))
            .context("no home directory for ~/.codex/auth.json")?;
        let auth: Value = serde_json::from_str(
            &std::fs::read_to_string(&auth_path)
                .context("could not read ~/.codex/auth.json — run `codex login` first")?,
        )?;
        let token = auth
            .pointer("/tokens/access_token")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .context("~/.codex/auth.json has no access token — run `codex login`")?;
        let account = auth
            .pointer("/tokens/account_id")
            .and_then(Value::as_str)
            .map(str::to_owned);
        Ok((token, account))
    }

    /// The cloud environment `codex cloud exec --env` requires, discovered
    /// from the repo's slug. A repo can have several environments — a
    /// pinned one wins, else the first.
    fn environment_for(ctx: &LaunchContext, slug: &str) -> anyhow::Result<(String, String)> {
        let (token, account) = Self::auth_token(ctx)?;
        let mut headers = vec![
            ("Authorization".into(), format!("Bearer {token}")),
            ("Content-Type".into(), "application/json".into()),
        ];
        if let Some(account) = account {
            headers.push(("ChatGPT-Account-Id".into(), account));
        }
        let response = http::request(
            "GET",
            &format!("https://chatgpt.com/backend-api/wham/environments/by-repo/github/{slug}"),
            &headers,
            None,
        )?;
        let environments = response
            .body
            .get("environments")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let pick = environments
            .iter()
            .find(|env| {
                env.get("isPinned")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
            })
            .or_else(|| environments.first())
            .with_context(|| {
                format!(
                    "no Codex Cloud environment exists for {slug} — \
                     create one at https://chatgpt.com/codex/environments"
                )
            })?;
        let id = pick
            .get("id")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .context("Codex Cloud environment entry had no id")?;
        let label = pick
            .get("label")
            .and_then(Value::as_str)
            .unwrap_or(slug)
            .to_owned();
        Ok((id, label))
    }
}

impl CloudBackend for CodexCloud {
    fn launch(
        &self,
        ctx: &LaunchContext,
        target: &CloudTarget,
        prompt: &str,
    ) -> anyhow::Result<CloudLaunch> {
        let slug = target.github_slug.clone().context(
            "Codex Cloud tasks need a GitHub remote — this repository's `origin` is not GitHub",
        )?;
        let (environment_id, environment_label) = Self::environment_for(ctx, &slug)?;
        let output = Self::run(
            ctx,
            &[
                "cloud",
                "exec",
                "--env",
                &environment_id,
                "--branch",
                &target.base_branch,
                prompt,
            ],
            None,
        )?;
        let url = super::find_url(&output, "codex")
            .context("codex cloud exec did not report a task URL")?;
        let task_id = super::url_tail(&url).context("Codex task URL had no id")?;
        Ok(CloudLaunch {
            cursor: ProviderResumeCursor::Codex { thread_id: task_id },
            url: Some(url),
            detail: Some(format!("environment {environment_label}")),
            extra: json!({ "environment_id": environment_id }),
            process: None,
        })
    }

    fn watch(&self, launch: &CloudLaunch, sink: BackendSink, stop: Arc<AtomicBool>) {
        let ProviderResumeCursor::Codex { thread_id } = &launch.cursor else {
            return;
        };
        let task_id = thread_id.clone();
        let environment_id = launch
            .extra
            .get("environment_id")
            .and_then(Value::as_str)
            .map(str::to_owned);
        let _ = sink.send(BackendEvent::Running {
            detail: launch.url.clone(),
        });
        while !stop.load(Ordering::Relaxed) {
            thread::sleep(POLL_INTERVAL);
            if stop.load(Ordering::Relaxed) {
                return;
            }
            // `status` exits non-zero until the task is READY — its stdout
            // still names the live state.
            let status = crate::command_env::search_path_command("codex")
                .args(["cloud", "status", &task_id])
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .output();
            match status {
                Ok(output) if output.status.success() => {
                    let link = launch.url.clone().unwrap_or_else(|| task_id.clone());
                    let _ = sink.send(BackendEvent::Finished {
                        success: true,
                        summary: format!(
                            "Codex Cloud task finished — {link}\nReview or apply it with `codex cloud diff {task_id}`"
                        ),
                    });
                    return;
                }
                Ok(output) => {
                    let text = format!(
                        "{}{}",
                        String::from_utf8_lossy(&output.stdout),
                        String::from_utf8_lossy(&output.stderr)
                    );
                    if text.contains("not found") || text.contains("Not Found") {
                        let _ = sink.send(BackendEvent::Finished {
                            success: false,
                            summary: "the Codex Cloud task is gone".to_owned(),
                        });
                        return;
                    }
                    let _ = sink.send(BackendEvent::Running {
                        detail: environment_id
                            .as_ref()
                            .map(|id| format!("environment {id}")),
                    });
                }
                Err(error) => {
                    let _ = sink.send(BackendEvent::Finished {
                        success: false,
                        summary: format!("could not poll the Codex Cloud task — {error:#}"),
                    });
                    return;
                }
            }
        }
    }

    fn launch_from_cursor(&self, cursor: &ProviderResumeCursor) -> Option<CloudLaunch> {
        let ProviderResumeCursor::Codex { thread_id } = cursor else {
            return None;
        };
        if thread_id.is_empty() {
            return None;
        }
        Some(CloudLaunch {
            cursor: cursor.clone(),
            url: Some(format!("https://chatgpt.com/codex/tasks/{thread_id}")),
            detail: None,
            extra: Value::Null,
            process: None,
        })
    }
}
