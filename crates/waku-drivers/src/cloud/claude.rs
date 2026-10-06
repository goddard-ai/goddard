//! Claude Code on the web, dispatched through `claude --cloud`.
//!
//! The CLI clones the repo's pushed state on Anthropic's infrastructure and
//! prints the session URL — but there is no programmatic status surface for
//! the resulting session, so this backend is dispatch-only: Goddard submits
//! the task, reports the link, and settles the turn. Watching and
//! continuing it happen on claude.ai.

use std::io::Read;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use anyhow::{Context as _, bail};

use super::{
    BackendEvent, BackendSink, CloudBackend, CloudLaunch, CloudTarget, LaunchContext, find_url,
    url_tail,
};
use crate::model::ProviderResumeCursor;

/// `claude --cloud` prints the session URL once the task exists; the read
/// window is generous because the CLI links GitHub before streaming.
const DISPATCH_TIMEOUT: Duration = Duration::from_secs(120);

pub(crate) struct ClaudeCloud;

impl CloudBackend for ClaudeCloud {
    fn launch(
        &self,
        ctx: &LaunchContext,
        target: &CloudTarget,
        prompt: &str,
    ) -> anyhow::Result<CloudLaunch> {
        if target.github_slug.is_none() {
            bail!(
                "Claude Code on the web needs a GitHub remote — this repository's `origin` is not GitHub"
            );
        }
        let mut child = crate::command_env::search_path_command(&ctx.binary)
            .arg("--cloud")
            .arg(prompt)
            .current_dir(&ctx.cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("failed to launch `claude --cloud`")?;
        let deadline = Instant::now() + DISPATCH_TIMEOUT;
        let mut output = String::new();
        let mut url = None;
        // Read stdout until the session URL shows; the CLI may keep the
        // process attached afterwards, which is why this caps the wait.
        let mut stdout = child.stdout.take().expect("stdout piped");
        let mut byte = [0u8; 1];
        while Instant::now() < deadline && url.is_none() {
            match stdout.read(&mut byte) {
                Ok(1) => {
                    output.push(byte[0] as char);
                    url = find_url(&output, "claude.ai");
                }
                _ => break,
            }
        }
        // The URL may only flush when the process exits — rescan everything
        // captured, not just the early stream. A failure reports stderr.
        let url = match url.or_else(|| find_url(&output, "claude.ai")) {
            Some(url) => url,
            None => {
                let _ = child.wait();
                let mut stderr = String::new();
                if let Some(mut pipe) = child.stderr.take() {
                    let _ = pipe.read_to_string(&mut stderr);
                }
                let detail = stderr.trim();
                bail!(
                    "`claude --cloud` did not report a session URL{}",
                    if detail.is_empty() {
                        String::new()
                    } else {
                        format!(" — {detail}")
                    }
                );
            }
        };
        // The dispatch is complete; whatever remains of the process is its
        // own streaming view — the remote session does not depend on it.
        let _ = child.kill();
        let _ = child.wait();
        let session_id = url_tail(&url).unwrap_or_default();
        Ok(CloudLaunch {
            cursor: ProviderResumeCursor::Claude {
                session_id,
                resume_at: None,
            },
            url: Some(url),
            detail: None,
            extra: serde_json::Value::Null,
            process: None,
        })
    }

    fn watch(&self, launch: &CloudLaunch, sink: BackendSink, _stop: Arc<AtomicBool>) {
        // Nothing programmatic to follow — settle the turn with the link.
        let link = launch.url.clone().unwrap_or_default();
        let _ = sink.send(BackendEvent::Finished {
            success: true,
            summary: format!("Dispatched to Claude Code on the web — {link}"),
        });
    }
}
