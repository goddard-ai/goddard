//! Provider-cloud execution backends.
//!
//! A task whose `SessionEnvironment` is `Cloud` does not run a provider CLI
//! here at all — the daemon submits the prompt to the provider's hosted
//! environment, which clones the repository's *pushed* state. Each backend
//! owns its transport: Devin and Cursor are HTTPS APIs, Codex rides
//! `codex cloud`, Claude dispatches through `claude --cloud`, Copilot through
//! `gh agent-task`, and Droid through `droid computer ssh`.
//!
//! The backends emit [`BackendEvent`]s on a dedicated watcher thread; the
//! cloud driver (`crate::driver::cloud`) folds them into `DriverEvent`s so a
//! cloud task is an ordinary Goddard session — transcript, lifecycle,
//! reattach — even where the provider only exposes coarse status.

mod claude;
mod codex;
mod copilot;
mod cursor;
mod devin;
mod droid;
pub(crate) mod http;

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use anyhow::{Context as _, bail};

use crate::model::{ProviderKind, ProviderResumeCursor};

/// The pushed-state context a cloud submission targets.
///
/// Cloud environments clone from the remote — local uncommitted work is
/// never shipped, which is the semantic the Environment menu promises.
/// `base_branch` is the checked-out branch when it tracks an upstream (so
/// it exists on the remote) and the remote's default branch otherwise.
pub(crate) struct CloudTarget {
    /// `owner/repo` when the remote is a GitHub remote. Codex, Copilot, and
    /// Claude key their environments to a GitHub slug; Devin and Cursor take
    /// the URL as-is.
    pub github_slug: Option<String>,
    /// HTTPS form of `repo_url` for providers that clone over HTTPS.
    pub https_url: String,
    /// The ref the cloud clone starts from.
    pub base_branch: String,
}

/// The handle a backend minted for a submitted task — enough to poll it,
/// steer it, and rebuild it from a persisted cursor after a daemon restart.
#[derive(Clone)]
pub(crate) struct CloudLaunch {
    /// Remote task identity, stored as the session's provider cursor so a
    /// daemon restart reattaches instead of resubmitting.
    pub cursor: ProviderResumeCursor,
    /// Browser-facing link for the remote task, when the provider has one.
    pub url: Option<String>,
    /// One-line provenance for the transcript (environment label, computer
    /// name, PR link).
    pub detail: Option<String>,
    /// Backend-private state the watcher needs — Codex's environment id,
    /// Cursor's current run id, Droid's child process. Not persisted; a
    /// reattached launch must recover it from `launch_from_cursor`.
    pub extra: serde_json::Value,
    /// A spawned child the watcher owns — backends whose cloud surface is a
    /// process stream (Droid's `computer ssh`) park the child here. `None`
    /// for pure-API backends.
    pub process: Option<Arc<ProcessSlot>>,
}

/// A spawned child process parked on a [`CloudLaunch`]. The watcher adopts
/// its stdout and kills the child when the driver stops watching.
pub(crate) struct ProcessSlot {
    child: parking_lot::Mutex<std::process::Child>,
    stdout: parking_lot::Mutex<Option<std::io::BufReader<std::process::ChildStdout>>>,
}

impl ProcessSlot {
    pub fn new(mut child: std::process::Child) -> Self {
        let stdout = child.stdout.take().map(std::io::BufReader::new);
        Self {
            child: parking_lot::Mutex::new(child),
            stdout: parking_lot::Mutex::new(stdout),
        }
    }

    pub fn take_stdout(&self) -> Option<std::io::BufReader<std::process::ChildStdout>> {
        self.stdout.lock().take()
    }

    pub fn kill(&self) {
        let _ = self.child.lock().kill();
    }

    pub fn wait(&self) {
        let _ = self.child.lock().wait();
    }
}

/// What a watcher observed. The driver maps these onto `DriverEvent`s —
/// it does not care whether they came from an API poll, an SSE stream, or
/// a child's stdout.
pub(crate) enum BackendEvent {
    /// The remote task is provisioned and working.
    Running { detail: Option<String> },
    /// The remote side is idle and can take a follow-up (Devin's
    /// `waiting_for_user`, a Cursor agent between runs).
    Waiting { detail: Option<String> },
    /// Provider-authored transcript text, appended in order.
    Message(String),
    /// A coarse progress row for providers without a message stream —
    /// the latest line replaces the last status detail.
    Progress(String),
    /// Terminal state. `summary` names where the work landed.
    Finished { success: bool, summary: String },
}

/// What a follow-up message did to the watch.
pub(crate) enum SendOutcome {
    /// The message joined the watched task — the watcher keeps reporting.
    Delivered,
    /// The follow-up minted a new remote unit (a Cursor run, a Codex
    /// task) — the driver swaps in this launch and starts a fresh watch.
    Respawned(CloudLaunch),
}

/// The local launch context CLI-backed backends shell out with.
pub(crate) struct LaunchContext {
    /// The provider's CLI binary, resolved by the usual binary probe.
    pub binary: std::path::PathBuf,
    /// The session workspace — where `git` reads the remote and branch.
    pub cwd: std::path::PathBuf,
}

/// The channel a backend watcher reports through. Sending is synchronous
/// and infallible while the driver lives — a dead receiver just means the
/// session was closed.
pub(crate) type BackendSink = crossbeam_channel::Sender<BackendEvent>;

/// A provider's hosted-environment surface. Blocking is expected: every
/// call runs on the driver's worker or watcher thread, never the dispatch
/// or render path.
pub(crate) trait CloudBackend: Send + Sync {
    /// Submit the task. `target` names the pushed state the provider clones.
    fn launch(
        &self,
        ctx: &LaunchContext,
        target: &CloudTarget,
        prompt: &str,
    ) -> anyhow::Result<CloudLaunch>;

    /// Watch a launched task until it settles or `stop` is set, reporting
    /// through `sink`. Runs on a dedicated thread per launch. Backends with
    /// no observable surface return without reporting — the driver treats a
    /// silent watch as unobservable, not dead.
    fn watch(&self, launch: &CloudLaunch, sink: BackendSink, stop: Arc<AtomicBool>);

    /// Whether the remote session accepts messages mid-flight — drives the
    /// driver's `supports_steer` and the idle-follow-up path.
    fn supports_messages(&self) -> bool {
        false
    }

    /// Whether a finished remote task still takes messages — Copilot's PR
    /// comment channel outlives the task's run.
    fn finished_accepts_messages(&self) -> bool {
        false
    }

    /// Deliver a follow-up message to the remote session. Backends without
    /// a message channel leave the default rejection.
    fn send_message(&self, _launch: &CloudLaunch, _text: &str) -> anyhow::Result<SendOutcome> {
        bail!("this cloud provider has no follow-up channel")
    }

    /// Ask the provider to stop the remote task. Default: nothing to stop.
    fn cancel(&self, _launch: &CloudLaunch) -> anyhow::Result<()> {
        Ok(())
    }

    /// Rebuild a launch handle from a persisted cursor — the reattach path.
    /// `None` means the provider has nothing a restart can watch.
    fn launch_from_cursor(&self, _cursor: &ProviderResumeCursor) -> Option<CloudLaunch> {
        None
    }
}

pub(crate) fn backend_for(provider: ProviderKind) -> Option<Arc<dyn CloudBackend>> {
    match provider {
        ProviderKind::Devin => Some(Arc::new(devin::DevinCloud::from_env())),
        ProviderKind::Codex => Some(Arc::new(codex::CodexCloud)),
        ProviderKind::Claude => Some(Arc::new(claude::ClaudeCloud)),
        ProviderKind::Cursor => Some(Arc::new(cursor::CursorCloud::from_env())),
        ProviderKind::Copilot => Some(Arc::new(copilot::CopilotCloud)),
        ProviderKind::Droid => Some(Arc::new(droid::DroidCloud)),
        _ => None,
    }
}

/// Resolve the repository context a cloud task will run against.
///
/// Cloud providers clone pushed state, so a launch only proceeds when the
/// checked-out branch is one the remote can actually see — the hard errors
/// below all name the state the remote would silently miss: no `origin`,
/// a detached HEAD, a never-pushed branch, or commits ahead of upstream.
/// Dirty worktree state is allowed through: uncommitted files simply stay
/// local, which is what "pushed state" means.
pub(crate) fn resolve_target(cwd: &Path) -> anyhow::Result<CloudTarget> {
    let snapshot = crate::git_branch::inspect(cwd)
        .context("could not inspect the repository")?
        .context("cloud tasks run against a Git repository — this workspace isn't one")?;
    let repo_url = snapshot
        .origin_url
        .clone()
        .context("cloud tasks run from pushed state — this repository has no `origin` remote")?;
    if snapshot.detached_head.is_some() {
        bail!("cloud tasks need a checked-out branch — HEAD is detached");
    }
    let branch = snapshot
        .current
        .clone()
        .context("cloud tasks need a checked-out branch")?;
    let Some(upstream) = &snapshot.upstream else {
        bail!("branch `{branch}` was never pushed — push it so the cloud environment can clone it");
    };
    if upstream.ahead > 0 {
        bail!(
            "{} unpushed commit{} on `{branch}` — push first; the cloud environment only sees pushed state",
            upstream.ahead,
            if upstream.ahead == 1 { "" } else { "s" },
        );
    }
    let https_url = https_url(&repo_url);
    Ok(CloudTarget {
        github_slug: github_slug(&https_url),
        https_url,
        base_branch: branch,
    })
}

/// `git@host:path` and `ssh://git@host/path` forms normalized to
/// `https://host/path` with a trailing `.git` stripped.
fn https_url(remote: &str) -> String {
    let url = if let Some(rest) = remote.strip_prefix("git@") {
        rest.replacen(':', "/", 1)
    } else if let Some(rest) = remote.strip_prefix("ssh://git@") {
        rest.to_owned()
    } else {
        remote.to_owned()
    };
    let url = url.strip_suffix(".git").unwrap_or(&url).to_owned();
    if url.starts_with("https://") || url.starts_with("http://") {
        url
    } else {
        format!("https://{url}")
    }
}

/// `owner/repo` when the URL is a GitHub remote (github.com or GHES hosts
/// still count — providers pass the URL through either way).
fn github_slug(https_url: &str) -> Option<String> {
    let path = https_url.strip_prefix("https://")?.splitn(2, '/').nth(1)?;
    let mut parts = path.split('/');
    Some(format!("{}/{}", parts.next()?, parts.next()?))
}

/// First `n` characters of a prompt as a session title.
pub(crate) fn prompt_title(prompt: &str) -> String {
    prompt.chars().take(80).collect()
}

/// Parse a URL or bare id out of a CLI's free-form stdout — cloud CLIs
/// print the task link rather than a structured id.
pub(crate) fn find_url(output: &str, needle: &str) -> Option<String> {
    output
        .split_whitespace()
        .map(|token| token.trim_matches(|c: char| "\"'<>()".contains(c)))
        .find(|token| token.starts_with("https://") && token.contains(needle))
        .map(str::to_owned)
}

/// The trailing id segment of a URL — `https://…/tasks/task-abc` → `task-abc`.
pub(crate) fn url_tail(url: &str) -> Option<String> {
    url.trim_end_matches('/')
        .rsplit('/')
        .next()
        .filter(|tail| !tail.is_empty())
        .map(str::to_owned)
}

pub(crate) fn env_var(name: &str) -> anyhow::Result<String> {
    std::env::var(name)
        .ok()
        .filter(|value| !value.is_empty())
        .with_context(|| format!("{name} is not set — set it in the environment and retry"))
}
