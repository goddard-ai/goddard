use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uuid::Uuid;

use crate::composer::{FileEntry, SlashCommand};
use crate::git::{
    AgentInvocation, BranchSnapshot, CheckoutStatus, CommitSnapshot, CreatedWorktree,
};
use crate::model::{Checkpoint, ProviderKind};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum ReviewDiffSource {
    LastTurn {
        session_id: Uuid,
        turn_id: Uuid,
        turn_count: usize,
    },
    Uncommitted,
    Unstaged,
    Staged,
    Committed,
    Branch,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ReviewDiffData {
    pub source: ReviewDiffSource,
    pub numstat: String,
    pub patch: String,
    pub complete_context: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct WorkingTreeEntry {
    pub relative_path: String,
    #[ts(type = "string")]
    pub absolute_path: PathBuf,
    pub name: String,
    pub is_dir: bool,
    pub expanded: bool,
    pub depth: usize,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum PullRequestState {
    Open,
    Closed,
    Merged,
}

/// Where a pull request's reviews stand, as the host summarises them. The
/// field itself is optional because some hosts report no rollup; absent means
/// the host did not say, not that a review is outstanding.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum PullRequestReviewDecision {
    Approved,
    ChangesRequested,
    ReviewRequired,
}

/// One pull request as the sidebar badge reads it. Fields past `is_draft` are
/// optional because a host read may omit them; absent renders as unknown, not
/// as a neutral value. Timestamps are unix seconds, matching session and turn
/// times.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PullRequestSummary {
    pub number: u64,
    pub title: String,
    pub url: String,
    pub state: PullRequestState,
    pub is_draft: bool,
    pub base_branch: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_decision: Option<PullRequestReviewDecision>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub additions: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deletions: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum WorkspaceOperation {
    ListTree {
        #[ts(type = "string")]
        root: PathBuf,
        #[ts(type = "string[]")]
        expanded_paths: Vec<PathBuf>,
    },
    BrowseDirectory {
        #[ts(type = "string | null")]
        path: Option<PathBuf>,
    },
    ReadTextFile {
        #[ts(type = "string")]
        root: PathBuf,
        #[ts(type = "string")]
        relative_path: PathBuf,
    },
    WriteTextFile {
        #[ts(type = "string")]
        root: PathBuf,
        #[ts(type = "string")]
        relative_path: PathBuf,
        content: String,
    },
    ListProjectFiles {
        #[ts(type = "string")]
        root: PathBuf,
        cap: usize,
    },
    DiscoverSlashCommands {
        provider: ProviderKind,
        #[ts(type = "string")]
        project_root: PathBuf,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        binary_override: Option<String>,
    },
    CreateProjectlessWorkspace {
        prompt: Option<String>,
    },
    MigrateProjectlessWorkspace {
        #[ts(type = "string")]
        path: PathBuf,
    },
    InspectBranches {
        #[ts(type = "string")]
        cwd: PathBuf,
    },
    CheckoutBranch {
        #[ts(type = "string")]
        cwd: PathBuf,
        branch: String,
        create: bool,
    },
    CreateWorktree {
        #[ts(type = "string")]
        project_path: PathBuf,
        /// User-chosen worktree name. When `None`, the daemon derives one
        /// from `prompt`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        /// The submitted prompt, used to derive a name when `name` is `None`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        prompt: Option<String>,
        /// Ref the worktree detaches at; `None` resolves the repository's
        /// default branch.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        base_ref: Option<String>,
    },
    /// Create a linked worktree that adopts `project_path`'s checkout state:
    /// based on its HEAD commit with uncommitted — including untracked —
    /// files carried over unstaged. Used to move a session that started in
    /// the ordinary checkout into a worktree without losing its work in
    /// progress; the source checkout keeps its own copy. Returns
    /// `WorktreeCreated`.
    CreateWorktreeFromCheckout {
        #[ts(type = "string")]
        project_path: PathBuf,
        /// User-chosen worktree name. When `None`, the daemon derives one
        /// from `prompt`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        /// Text used to derive a name when `name` is `None` — the session's
        /// title or first prompt.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        prompt: Option<String>,
    },
    /// Remove a linked worktree created by `CreateWorktree`. Git refuses to
    /// remove a dirty worktree, making this safe to call on abandonment.
    /// `force` overrides that refusal — only for worktrees whose content is
    /// a discardable copy or fully captured in a ref.
    RemoveWorktree {
        #[ts(type = "string")]
        path: PathBuf,
        /// `false` matches older clients that did not send the field.
        #[serde(default)]
        force: bool,
    },
    /// Recreate a worktree's directory when it was deleted outside the app.
    /// `path` is the session's stored project path inside the worktree; the
    /// call is a no-op while it still exists. A recreated worktree checks
    /// out `branch` when it still exists and otherwise comes up detached at
    /// `base_ref` — typically the session's latest checkpoint — falling back
    /// to the repository's default branch.
    EnsureWorktree {
        #[ts(type = "string")]
        project_path: PathBuf,
        #[ts(type = "string")]
        path: PathBuf,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        branch: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        base_ref: Option<String>,
    },
    InspectCommit {
        #[ts(type = "string")]
        cwd: PathBuf,
    },
    /// Lightweight dirty/unpushed status for sidebar badges; cheaper than
    /// `InspectCommit`, which also computes diff numstats.
    InspectCheckoutStatus {
        #[ts(type = "string")]
        cwd: PathBuf,
    },
    GenerateCommitMessage {
        #[ts(type = "string")]
        cwd: PathBuf,
        include_unstaged: bool,
        invocation: AgentInvocation,
    },
    Commit {
        #[ts(type = "string")]
        cwd: PathBuf,
        message: String,
        include_unstaged: bool,
        push: bool,
    },
    Push {
        #[ts(type = "string")]
        cwd: PathBuf,
    },
    CaptureTurnStart {
        #[ts(type = "string")]
        cwd: PathBuf,
        session_id: Uuid,
        turn_count: usize,
    },
    CaptureTurn {
        #[ts(type = "string")]
        cwd: PathBuf,
        session_id: Uuid,
        turn_count: usize,
    },
    CaptureRef {
        #[ts(type = "string")]
        cwd: PathBuf,
        git_ref: String,
    },
    RestoreRef {
        #[ts(type = "string")]
        cwd: PathBuf,
        git_ref: String,
    },
    HasRef {
        #[ts(type = "string")]
        cwd: PathBuf,
        git_ref: String,
    },
    SessionTurnRefs {
        #[ts(type = "string")]
        cwd: PathBuf,
        session_id: Uuid,
    },
    DeleteRef {
        #[ts(type = "string")]
        cwd: PathBuf,
        git_ref: String,
    },
    DeleteTurnRefsAfter {
        #[ts(type = "string")]
        cwd: PathBuf,
        session_id: Uuid,
        retained_turn_count: usize,
        previous_turn_count: usize,
    },
    DeleteSessionRefs {
        #[ts(type = "string")]
        cwd: PathBuf,
        session_id: Uuid,
    },
    CopySessionRefs {
        #[ts(type = "string")]
        cwd: PathBuf,
        source_session_id: Uuid,
        target_session_id: Uuid,
        through_turn_count: usize,
    },
    CollectReviewDiff {
        #[ts(type = "string")]
        cwd: PathBuf,
        source: ReviewDiffSource,
    },
    /// Pull requests whose head branch is `head_branch`, as the repository's
    /// host reports them through its CLI. Read-only; the host is never
    /// written to.
    ListPullRequests {
        #[ts(type = "string")]
        cwd: PathBuf,
        head_branch: String,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum WorkspaceResult {
    Ack,
    WorkingTree {
        entries: Vec<WorkingTreeEntry>,
    },
    Directory {
        #[ts(type = "string")]
        path: PathBuf,
        #[ts(type = "string | null")]
        parent: Option<PathBuf>,
        #[ts(type = "string")]
        home: PathBuf,
        #[ts(type = "string")]
        filesystem_root: PathBuf,
        entries: Vec<WorkingTreeEntry>,
    },
    TextFile {
        content: String,
    },
    ProjectFiles {
        entries: Vec<FileEntry>,
    },
    SlashCommands {
        commands: Vec<SlashCommand>,
    },
    ProjectlessWorkspace {
        #[ts(type = "string")]
        cwd: PathBuf,
    },
    Branches {
        snapshot: Option<BranchSnapshot>,
    },
    BranchChanged {
        snapshot: BranchSnapshot,
    },
    WorktreeCreated {
        worktree: CreatedWorktree,
    },
    /// `created` is true when the worktree directory had to be recreated.
    /// `branch` reports the checkout it came up in — `None` for a detached
    /// HEAD — and is only meaningful when `created`.
    WorktreeEnsured {
        created: bool,
        branch: Option<String>,
    },
    CommitSnapshot {
        snapshot: CommitSnapshot,
    },
    /// `None` when `cwd` is not inside a Git repository.
    CheckoutStatus {
        status: Option<CheckoutStatus>,
    },
    CommitMessage {
        message: String,
    },
    Checkpoint {
        checkpoint: Checkpoint,
    },
    Bool {
        value: bool,
    },
    TurnRefs {
        turn_counts: Vec<usize>,
    },
    ReviewDiff {
        data: ReviewDiffData,
    },
    /// `None` when the host could not be read — its CLI is missing,
    /// unauthenticated, or the directory is not a repository it knows — which
    /// is different from `Some(vec![])`, a host that answered "no pull
    /// requests".
    PullRequests {
        entries: Option<Vec<PullRequestSummary>>,
    },
}
