use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::model::ProviderKind;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct BranchEntry {
    pub name: String,
    pub checked_out_elsewhere: bool,
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
    /// The pull stopped on conflicts and an integration is still in progress.
    Conflict {
        in_progress: SyncInProgress,
        /// Working-tree paths still carrying conflict markers.
        files: Vec<String>,
    },
}

/// How a `Land` operation ended. `base` names the branch it resolved, which
/// the conflict modal and prompts quote back to the user.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum LandOutcome {
    /// The checkout's commits are on the base, which was fast-forwarded to
    /// this HEAD.
    Landed { base: String },
    /// The rebase or merge stopped on conflicts; the integration is still in
    /// progress and the checkout owns the conflict markers.
    Conflict {
        base: String,
        in_progress: SyncInProgress,
        /// Working-tree paths still carrying conflict markers.
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

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
pub struct CreatedWorktree {
    #[ts(type = "string")]
    pub path: PathBuf,
    /// The linked worktree's directory name — its identity in clients.
    pub name: String,
}
