use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::model::ProviderKind;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct BranchEntry {
    pub name: String,
    pub checked_out_elsewhere: bool,
    /// Committer date of the branch tip, unix seconds (`%(committerdate:unix)`).
    /// Feeds the picker's recency ranking; `None` only if Git reported no date.
    pub last_commit_at: Option<u64>,
}

/// The checked-out branch's relationship to its configured upstream —
/// `origin/<branch>` in a typical checkout. Counts come from the local
/// remote-tracking ref, so they describe the last fetch, not the remote's
/// current state.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct UpstreamStatus {
    /// The tracking ref's short name, e.g. `origin/main`.
    pub name: String,
    /// Commits on HEAD the upstream does not have.
    pub ahead: u64,
    /// Commits on the upstream HEAD does not have.
    pub behind: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct BranchSnapshot {
    #[ts(type = "string")]
    pub repository: PathBuf,
    pub current: Option<String>,
    pub detached_head: Option<String>,
    pub default_branch: Option<String>,
    /// The fetch URL of the `origin` remote, if one is configured.
    pub origin_url: Option<String>,
    /// `None` for a detached HEAD or a branch with no upstream configured.
    pub upstream: Option<UpstreamStatus>,
    pub branches: Vec<BranchEntry>,
    pub additions: u64,
    pub deletions: u64,
}

impl BranchSnapshot {
    pub fn display_branch(&self) -> Option<&str> {
        self.current.as_deref().or(self.detached_head.as_deref())
    }
}

/// Where a workspace file is reachable on the repository's `origin` remote,
/// as of the last fetch — the pieces a host like GitHub needs to build a
/// `blob/` URL. Resolved from local remote-tracking refs, so it reflects
/// the last fetch rather than the remote's live state.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct RemoteFileRef {
    /// The URL's ref segment: the remote branch name (`origin/` stripped)
    /// for a tracked branch, or HEAD's full SHA for a pushed detached HEAD.
    pub reference: String,
    /// Repo-root-relative path — the workspace root can sit inside the
    /// repository, so this is not always the workspace-relative path.
    pub path: String,
}

/// Sidebar-grade checkout status: whether the working tree is dirty and how
/// many commits on HEAD are not reachable from any remote-tracking ref.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct CheckoutStatus {
    /// Staged, unstaged, or untracked changes are present.
    pub uncommitted_changes: bool,
    /// Commits on HEAD unreachable from every remote ref; always `0` when the
    /// repository has no remote configured.
    pub unpushed_commits: u64,
}

/// One working-tree entry as `git status --porcelain=v1` reports it.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct StatusEntry {
    /// The two-letter porcelain status, e.g. `M`, `A`, `??`.
    pub status: String,
    pub path: String,
}

/// Everything archiving a session's checkout would stash away: every dirty
/// working-tree file and the subjects of the commits on HEAD that no
/// remote-tracking ref has. Powers the archive confirmation.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct ArchivePreview {
    pub files: Vec<StatusEntry>,
    pub unpushed_commits: Vec<String>,
}

/// One file with uncommitted changes in the Git panel's staged or unstaged
/// section. A partially staged file appears in both lists, once per section.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct GitFileChange {
    pub path: String,
    /// The section's porcelain letter for this file (`M`, `A`, `D`, `R`,
    /// `??`), so the UI can distinguish an add from a delete without
    /// re-parsing the diff.
    pub status: String,
    pub additions: u64,
    pub deletions: u64,
    /// Listed under `unstaged` via `??`. Untracked files have no index entry,
    /// so `additions`/`deletions` stay zero and their diff preview is
    /// synthesized per file.
    pub untracked: bool,
}

/// The Git panel's working-tree state: branch, upstream relationship, and
/// both change lists in one pass.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct GitPanelSnapshot {
    /// Current branch name, or the short HEAD when detached.
    pub branch: String,
    /// The fetch URL of the `origin` remote, if one is configured.
    pub origin_url: Option<String>,
    /// `None` for a detached HEAD or a branch with no upstream configured.
    pub upstream: Option<UpstreamStatus>,
    /// A remote the branch could publish to exists — the same condition the
    /// commit dialog's push affordance uses.
    pub can_push: bool,
    /// The commit where HEAD diverges from the resolved base branch, for the
    /// log's two-lane graph; `None` when no base resolves.
    pub merge_base: Option<String>,
    /// Where `Land` would send this checkout's commits — `None` when no base
    /// branch resolves or HEAD has no commits the base lacks.
    pub land_target: Option<LandTarget>,
    pub staged: Vec<GitFileChange>,
    pub unstaged: Vec<GitFileChange>,
}

/// The base branch `Land` resolved for a checkout, and how far ahead of it
/// HEAD is. Only reported while `ahead` is nonzero — landing a checkout with
/// nothing new is a no-op the panel never offers.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct LandTarget {
    /// The resolved local branch name.
    pub branch: String,
    /// Commits on HEAD the base lacks.
    pub ahead: u64,
}

/// One `git log` entry for the panel's commit list.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct CommitEntry {
    pub sha: String,
    pub short_sha: String,
    pub subject: String,
    /// Message body past the subject; empty when the commit has none.
    pub body: String,
    /// Author name (`%an`).
    pub author: String,
    /// Author email (`%ae`); feeds the avatar lookup in the UI.
    pub author_email: String,
    /// Author date, unix seconds (`%at`).
    pub authored_at: u64,
    /// Lines the commit touched; both zero for merges and binary-only diffs.
    pub additions: u64,
    pub deletions: u64,
}

/// How `PullUpstream` integrates upstream commits.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum PullStrategy {
    Rebase,
    Merge,
}

/// A reviewer's recorded verdict on a `qa` commit — one line of the
/// commit's `refs/notes/qa` note.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum ReviewDecision {
    Approved,
    Rejected,
}

/// One line of a commit's `refs/notes/qa` note. `reviewer` is the pusher's
/// git identity (`user.name <user.email>`) — the note itself is trusted via
/// the `origin` push, not a signature.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ReviewRecord {
    pub reviewer: String,
    pub decision: ReviewDecision,
    /// Unix seconds.
    pub at: u64,
}

/// One proposed commit on the QA branch and its review state, for the
/// Projects page's Review tab.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ReviewEntry {
    pub commit: CommitEntry,
    /// `Test-Plan:` trailer lines — the checklist a human verifies.
    pub test_plans: Vec<String>,
    /// Policy verdict: a trailer or a sensitive path means a human must
    /// approve before this can promote. Otherwise it counts as approved
    /// without a note.
    pub needs_review: bool,
    /// Latest decision per reviewer, one record each.
    pub reviews: Vec<ReviewRecord>,
    /// Some reviewer's latest decision is `Rejected`.
    pub rejected: bool,
    /// A `git revert` of this commit sits later on the QA branch — its
    /// changes are undone, so it can't block the frontier.
    pub reverted: bool,
    /// Promotable: no pending rejection, and either policy auto-approves it
    /// or a reviewer approved it.
    pub approved: bool,
}

/// The QA branch's proposed commits (oldest first) and how far the base
/// branch may fast-forward — the longest prefix where every entry is
/// approved.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ReviewQueue {
    /// The promotion target — `origin/<default branch>`, typically `main`.
    pub base_branch: Option<String>,
    /// The branch the queue was read from — the daemon's configured QA
    /// branch — so clients name it instead of assuming `qa`.
    #[serde(default)]
    pub review_branch: String,
    /// Oldest first. Empty when the repo has no `origin/<review_branch>`.
    pub entries: Vec<ReviewEntry>,
    /// Commit the base branch can fast-forward to — the last entry of the
    /// approved prefix. `None` when nothing is promotable.
    pub frontier: Option<String>,
}

/// Which integration left the checkout conflicted — decides whether abort
/// runs `git rebase --abort` or `git merge --abort`.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum SyncInProgress {
    Rebase,
    Merge,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum PullOutcome {
    /// The pull applied cleanly; the checkout is caught up.
    Clean,
    /// The upstream had nothing HEAD lacked — the fetch ran but no
    /// integration did. `upstream` is the tracking ref the check measured
    /// against ("origin/main").
    UpToDate { upstream: String },
    /// The pull stopped on conflicts and an integration is still in progress.
    Conflict {
        in_progress: SyncInProgress,
        /// Working-tree paths still carrying conflict markers. `[]` matches
        /// older writers that did not send the field.
        #[serde(default)]
        files: Vec<String>,
    },
}

/// A base branch's relationship to its remote tracking branch — what the
/// transcript's landed notice and the draft's sync strip read. A read,
/// not a fetch: the counts run against the last-known remote-tracking ref.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct BasePushState {
    /// `<base>@{upstream}` ("origin/main"); `None` when the branch tracks
    /// no remote branch or does not exist.
    pub upstream: Option<String>,
    /// Commits on the base its upstream lacks; `None` when the upstream is
    /// configured but its remote-tracking ref does not resolve locally —
    /// unknown, not zero.
    pub ahead: Option<u64>,
    /// Commits on the upstream the base lacks; the same `None` semantics
    /// as `ahead`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub behind: Option<u64>,
}

/// How a `PushBase` operation ended. `base` and `upstream` echo the
/// resolved refs so toasts and the failure modal can name them.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum PushBaseOutcome {
    /// The upstream now contains every commit on the base.
    Pushed { base: String, upstream: String },
    /// The base tracks no remote branch — there is nowhere to push.
    NoUpstream { base: String },
    /// The upstream already contains every commit on the base.
    UpToDate { base: String, upstream: String },
    /// The remote refused a non-fast-forward update: its branch carries
    /// commits the base lacks, so it must be synced first. `message` is
    /// Git's own output for the failure modal's detail block.
    Rejected {
        base: String,
        upstream: String,
        message: String,
    },
}

/// How a `RebaseOnto` operation ended. `base` names the branch the checkout
/// was being moved onto, which the conflict modal and prompts quote back to
/// the user.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum RebaseOutcome {
    /// HEAD now sits on top of `base` — the replay or merge completed.
    Rebased { base: String },
    /// The rebase or merge stopped on conflicts; the integration is still
    /// in progress and the checkout owns the conflict markers.
    Conflict {
        base: String,
        in_progress: SyncInProgress,
        /// Working-tree paths still carrying conflict markers. `[]` matches
        /// older writers that did not send the field.
        #[serde(default)]
        files: Vec<String>,
    },
}

/// How a `Land` operation ended. `base` names the branch it resolved, which
/// the conflict modal and prompts quote back to the user.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum LandOutcome {
    /// The checkout's commits are on the base, which was fast-forwarded to
    /// this HEAD. `commits` lists what landed, newest first, capped at the
    /// land operation's collect limit — `ahead` is the true total.
    Landed {
        base: String,
        commits: Vec<CommitEntry>,
        ahead: u64,
    },
    /// The base already contains every commit on the checkout — the land
    /// either already ran or there was never anything to send. A neutral
    /// result, not an error.
    AlreadyLanded { base: String },
    /// The rebase or merge stopped on conflicts; the integration is still in
    /// progress and the checkout owns the conflict markers.
    Conflict {
        base: String,
        in_progress: SyncInProgress,
        /// Working-tree paths still carrying conflict markers. `[]` matches
        /// older writers that did not send the field.
        #[serde(default)]
        files: Vec<String>,
    },
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct CommitSnapshot {
    pub branch: String,
    pub additions: u64,
    pub deletions: u64,
    pub staged_additions: u64,
    pub staged_deletions: u64,
    pub has_staged: bool,
    pub has_unstaged: bool,
    pub can_push: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
pub struct AgentInvocation {
    pub provider: ProviderKind,
    #[ts(type = "string")]
    pub binary: PathBuf,
    pub model: Option<String>,
    pub reasoning_effort: Option<String>,
}

// A commit subject is a fixed classification over a diff that is already in
// the prompt, so it does not need — or benefit from — the model the task
// runs on. These providers pin generation to a named cheap tier.
pub const CLAUDE_COMMIT_MODEL: &str = "claude-haiku-4-5";
pub const CODEX_COMMIT_MODEL: &str = "gpt-5.6-luna";

/// The model a commit-message generation actually invokes: the pinned tier
/// for providers that pin one, otherwise the requested model.
pub fn commit_generation_model<'a>(
    provider: ProviderKind,
    requested: Option<&'a str>,
) -> Option<&'a str> {
    match provider {
        ProviderKind::Claude => Some(CLAUDE_COMMIT_MODEL),
        ProviderKind::Codex => Some(CODEX_COMMIT_MODEL),
        _ => requested,
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct CreatedWorktree {
    #[ts(type = "string")]
    pub path: PathBuf,
    /// The linked worktree's directory name — its identity in clients.
    pub name: String,
    /// LFS-tracked files materialized as pointer stubs because the `lfs`
    /// filters could not run — `git lfs pull` in the worktree fetches the
    /// real content.
    #[serde(default)]
    pub lfs_skipped: bool,
}
