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
