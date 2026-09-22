//! Factory's Droid Computers — persistent managed machines reached through
//! `droid computer ssh`. The task runs as a remote `droid exec` whose
//! stream-json output is the transcript; killing the tunnel ends it.
//!
//! Unlike the API backends there is no task id to reattach — the remote
//! exec is bound to the ssh session, so a daemon restart cannot resume it.

use std::io::BufRead;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Context as _, bail};
use serde_json::{Value, json};

use super::{
    BackendEvent, BackendSink, CloudBackend, CloudLaunch, CloudTarget, LaunchContext, ProcessSlot,
};
use crate::model::ProviderResumeCursor;

pub(crate) struct DroidCloud;

impl DroidCloud {
    /// An active managed computer's name — `droid computer list` output is
    /// a table of `name  id  status  …` rows; the header is skipped and any
    /// row marked active/ready wins.
    fn pick_computer(ctx: &LaunchContext) -> anyhow::Result<String> {
        let output = crate::command_env::search_path_command(&ctx.binary)
            .args(["computer", "list"])
            .current_dir(&ctx.cwd)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .context("failed to run `droid computer list`")?;
        if !output.status.success() {
            bail!(
                "`droid computer list` failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        let text = String::from_utf8_lossy(&output.stdout);
        for line in text.lines().skip(1) {
            let columns: Vec<&str> = line.split_whitespace().collect();
            if columns.len() >= 2
                && columns.iter().any(|col| {
                    col.eq_ignore_ascii_case("active") || col.eq_ignore_ascii_case("ready")
                })
            {
                return Ok(columns[0].to_owned());
            }
        }
        bail!(
            "no active Droid Computer — create one in the Factory app \
             (Settings → Droid Computers) and retry"
        )
    }
}

impl CloudBackend for DroidCloud {
    fn launch(
        &self,
        ctx: &LaunchContext,
        target: &CloudTarget,
        prompt: &str,
    ) -> anyhow::Result<CloudLaunch> {
        let computer = Self::pick_computer(ctx)?;
        // The machine starts blank for this task — cloning the pushed repo
        // is part of the instruction, same as every other cloud provider.
        let remote_command = format!(
            "droid exec --output-format stream-json {}",
            shell_quote(&format!(
                "Clone {} and check out the {} branch, then:\n\n{}",
                target.https_url, target.base_branch, prompt
            ))
        );
        let child = crate::command_env::search_path_command(&ctx.binary)
            .args(["computer", "ssh", &computer, "--", &remote_command])
            .current_dir(&ctx.cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .with_context(|| format!("failed to reach Droid Computer {computer}"))?;
        Ok(CloudLaunch {
            cursor: ProviderResumeCursor::Droid {
                // Nothing survives the tunnel — the id is the machine so the
                // cursor at least names where the work ran.
                session_id: computer.clone(),
            },
            url: None,
            detail: Some(format!("Droid Computer {computer}")),
            extra: json!({}),
            process: Some(Arc::new(ProcessSlot::new(child))),
        })
    }

    fn watch(&self, launch: &CloudLaunch, sink: BackendSink, stop: Arc<AtomicBool>) {
        let Some(process) = launch.process.clone() else {
            return;
        };
        let Some(mut reader) = process.take_stdout() else {
            return;
        };
        let mut line = String::new();
        loop {
            if stop.load(Ordering::Relaxed) {
                process.kill();
                process.wait();
                return;
            }
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    let Ok(event) = serde_json::from_str::<Value>(line.trim()) else {
                        continue;
                    };
                    let kind = event
                        .get("type")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    match kind {
                        "assistant" | "message" => {
                            if let Some(text) = event
                                .pointer("/message/content/0/text")
                                .or_else(|| event.get("text"))
                                .and_then(Value::as_str)
                            {
                                let _ = sink.send(BackendEvent::Message(text.to_owned()));
                            }
                        }
                        "tool_call" | "tool" => {
                            if let Some(name) = event
                                .get("name")
                                .or_else(|| event.get("tool"))
                                .and_then(Value::as_str)
                            {
                                let _ = sink.send(BackendEvent::Progress(name.to_owned()));
                            }
                        }
                        "result" => {
                            let success = event
                                .get("is_error")
                                .and_then(Value::as_bool)
                                .map(|error| !error)
                                .unwrap_or(true);
                            let summary: String = event
                                .get("result")
                                .or_else(|| event.get("text"))
                                .and_then(Value::as_str)
                                .unwrap_or("the Droid task ended")
                                .chars()
                                .take(400)
                                .collect();
                            let _ = sink.send(BackendEvent::Finished { success, summary });
                            process.wait();
                            return;
                        }
                        _ => {}
                    }
                }
            }
        }
        process.wait();
        let _ = sink.send(BackendEvent::Finished {
            success: true,
            summary: format!(
                "the Droid exec on {} ended",
                launch.detail.as_deref().unwrap_or("the computer")
            ),
        });
    }

    fn cancel(&self, launch: &CloudLaunch) -> anyhow::Result<()> {
        // Killing the tunnel ends the remote exec and unblocks the
        // watcher's read.
        if let Some(process) = &launch.process {
            process.kill();
        }
        Ok(())
    }
}

/// Wrap a remote-side argument in single quotes.
fn shell_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}
