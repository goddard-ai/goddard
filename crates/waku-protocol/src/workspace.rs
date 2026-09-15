use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uuid::Uuid;

use crate::composer::{FileEntry, SlashCommand};
use crate::git::{
    AgentInvocation, ArchivePreview, BranchSnapshot, CheckoutStatus, CommitEntry, CommitSnapshot,
    CreatedWorktree, GitPanelSnapshot, PullOutcome, PullStrategy,
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
    /// Marks a `CommitDiff` result's data; `CollectReviewDiff` never receives
    /// it because a commit diff comes from `git show`, not a range diff.
    Commit,
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

/// A pull request's checks rolled up to one signal: any failure reports
/// `Failing`, otherwise any unfinished check reports `Pending`. Absent means
/// the host reported no checks at all, which renders as nothing rather than
/// as passing.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum PullRequestCheckStatus {
    Passing,
    Pending,
    Failing,
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
    pub check_status: Option<PullRequestCheckStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub additions: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deletions: Option<u64>,
    /// Author login, when the read that produced this row asked for it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    /// Head branch name, when the read that produced this row asked for it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head_branch: Option<String>,
}

/// A GitHub repository as `gh` resolves it for a working directory. `host` is
/// `None` for github.com and names the GitHub Enterprise host otherwise.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct GitHubRepoRef {
    pub owner: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    pub web_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_branch: Option<String>,
}

/// Why a repo's GitHub reads cannot be answered. `Ready` pairs with a
/// resolved repo; the other variants tell the UI whether to hint at
/// installing `gh`, at authenticating, or to stay hidden (a repo `gh` knows
/// but that is not on GitHub).
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum GitHubAvailability {
    Ready,
    MissingCli,
    Unauthenticated,
}

/// Open/closed/all filter shared by issue and pull-request list reads.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum WorkItemQueryState {
    Open,
    Closed,
    All,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum IssueState {
    Open,
    Closed,
}

/// One issue as the GitHub browser's list reads it.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct IssueSummary {
    pub number: u64,
    pub title: String,
    pub url: String,
    pub state: IssueState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(default)]
    pub labels: Vec<String>,
    #[serde(default)]
    pub assignees: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<u64>,
}

/// One comment on an issue or pull request.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct WorkItemComment {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    pub body: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<u64>,
}

/// An issue with its body and comment thread.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct IssueDetail {
    pub summary: IssueSummary,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    #[serde(default)]
    pub comments: Vec<WorkItemComment>,
}

/// One file changed by a pull request.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PullRequestFile {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub additions: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deletions: Option<u64>,
}

/// One check run or commit status on a pull request's head. `run_id` is the
/// Actions run the check belongs to — what `gh run view` needs for its log —
/// recovered from the check's details URL.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PullRequestCheck {
    pub name: String,
    pub status: PullRequestCheckStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_seconds: Option<u64>,
}

/// A pull request with its body, comment thread, checks, and changed files.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PullRequestDetail {
    pub summary: PullRequestSummary,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    #[serde(default)]
    pub comments: Vec<WorkItemComment>,
    #[serde(default)]
    pub checks: Vec<PullRequestCheck>,
    #[serde(default)]
    pub files: Vec<PullRequestFile>,
}

/// One linked worktree or the repository's ordinary checkout, enriched with
/// status for the Projects page's worktree table. `None` fields mean the
/// read could not answer rather than a neutral value.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RepoWorktree {
    /// Absolute worktree path.
    #[ts(type = "string")]
    pub path: PathBuf,
    /// HEAD commit sha.
    pub head: String,
    /// Checked-out branch short name; `None` for detached HEAD.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    /// The repository's main (ordinary) working tree, never a linked one.
    pub is_main: bool,
    /// Tracked + untracked change count.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dirty_files: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ahead: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub behind: Option<u64>,
    /// Unix seconds of HEAD's commit time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_commit_at: Option<u64>,
}

/// One local branch or remote-tracking ref for the Projects page's branch
/// table. Remote entries carry `remote` ("origin") and `name` without the
/// remote prefix; local entries have `remote: None`.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RepoBranch {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote: Option<String>,
    pub sha: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upstream: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ahead: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub behind: Option<u64>,
    /// Unix seconds of the tip commit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_commit_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_commit_subject: Option<String>,
    /// Worktree path currently holding this branch checked out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(type = "string")]
    pub checked_out_in: Option<PathBuf>,
}

/// One branch `DeleteBranches` could not remove, with Git's own wording.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct BranchDeleteFailure {
    pub name: String,
    pub error: String,
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
        /// User-chosen worktree name. When `None`, the daemon generates a
        /// random one.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
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
        /// User-chosen worktree name. When `None`, the daemon generates a
        /// random one.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
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
    /// The repository's worktrees — the ordinary checkout and each linked
    /// one — with per-checkout status for the Projects page's worktree
    /// table. Returns `RepoWorktrees`; `None` outside a Git repository.
    ListWorktrees {
        #[ts(type = "string")]
        cwd: PathBuf,
    },
    /// Local branches and remote-tracking refs for the Projects page's
    /// branch table. Returns `RepoBranches`; `None` outside a Git
    /// repository.
    ListRepoBranches {
        #[ts(type = "string")]
        cwd: PathBuf,
    },
    /// `git fetch --prune <remote>`: refresh one remote's tracking refs and
    /// drop the ones it deleted.
    FetchRemote {
        #[ts(type = "string")]
        cwd: PathBuf,
        remote: String,
    },
    /// Delete local branches — `git branch -d` each, `-D` when `force`.
    /// A branch Git refuses, such as one checked out in a worktree or one
    /// unmerged without `force`, comes back in `BranchDeletions.failures`
    /// rather than failing the batch.
    DeleteBranches {
        #[ts(type = "string")]
        cwd: PathBuf,
        names: Vec<String>,
        /// `false` matches older clients that did not send the field.
        #[serde(default)]
        force: bool,
    },
    /// `git worktree prune`: drop worktree registrations whose directories
    /// were deleted outside the app.
    PruneWorktrees {
        #[ts(type = "string")]
        cwd: PathBuf,
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
    /// The dirty-file list and unpushed commit subjects an archive
    /// confirmation shows; `None` outside a work tree.
    InspectArchivePreview {
        #[ts(type = "string")]
        cwd: PathBuf,
    },
    GenerateCommitMessage {
        #[ts(type = "string")]
        cwd: PathBuf,
        include_unstaged: bool,
        invocation: AgentInvocation,
    },
    /// One-shot agent generation of a shell command for a terminal's
    /// command bar. `scrollback` is the client's recent terminal output and
    /// `shell` the PTY's shell name — both arrive already bounded.
    GenerateTerminalCommand {
        #[ts(type = "string")]
        cwd: PathBuf,
        request: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        scrollback: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        shell: Option<String>,
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
    /// The Git panel's one-pass working-tree read: branch, upstream counts,
    /// and both change lists. `None` outside a work tree.
    InspectGitPanel {
        #[ts(type = "string")]
        cwd: PathBuf,
    },
    StageFile {
        #[ts(type = "string")]
        cwd: PathBuf,
        path: String,
    },
    UnstageFile {
        #[ts(type = "string")]
        cwd: PathBuf,
        path: String,
    },
    /// Integrate upstream changes (`git pull --rebase` or `--no-rebase`).
    /// Conflict means an integration is still in progress; `AbortSync` or
    /// the agent has to resolve it before anything else can commit.
    PullUpstream {
        #[ts(type = "string")]
        cwd: PathBuf,
        strategy: PullStrategy,
    },
    /// Abort whichever integration is mid-flight (`rebase --abort` or
    /// `merge --abort`). No-op when none is.
    AbortSync {
        #[ts(type = "string")]
        cwd: PathBuf,
    },
    /// `git log` for HEAD, paged: `skip` leading entries are skipped and at
    /// most `limit` are returned.
    ListCommits {
        #[ts(type = "string")]
        cwd: PathBuf,
        skip: usize,
        limit: usize,
    },
    /// One file's working-tree diff for the Git panel's hover preview.
    /// `staged` selects `--cached`; an unstaged path with no index entry is
    /// diffed as a new file.
    FileDiff {
        #[ts(type = "string")]
        cwd: PathBuf,
        path: String,
        staged: bool,
    },
    /// One commit's diff (`git show`) for the Git panel's diff modal.
    CommitDiff {
        #[ts(type = "string")]
        cwd: PathBuf,
        sha: String,
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
    /// The GitHub repository `cwd` belongs to, via `gh repo view`. Returns
    /// `GitHubRepo`; `availability` explains an absent repo so the caller can
    /// hint at installing or authenticating `gh` rather than guessing.
    ResolveGitHubRepo {
        #[ts(type = "string")]
        cwd: PathBuf,
    },
    /// Repo-wide issue list for the GitHub browser. `query` is passed to
    /// `gh issue list --search`.
    ListIssues {
        #[ts(type = "string")]
        cwd: PathBuf,
        state: WorkItemQueryState,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        query: Option<String>,
    },
    /// One issue with body and comments.
    GetIssue {
        #[ts(type = "string")]
        cwd: PathBuf,
        number: u64,
    },
    /// Repo-wide pull-request list for the GitHub browser. The branch-scoped
    /// sidebar scan keeps using `ListPullRequests`.
    ListRepoPullRequests {
        #[ts(type = "string")]
        cwd: PathBuf,
        state: WorkItemQueryState,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        query: Option<String>,
    },
    /// One pull request with body, comments, checks, and changed files.
    GetPullRequest {
        #[ts(type = "string")]
        cwd: PathBuf,
        number: u64,
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
    /// `None` when `cwd` is not inside a Git repository.
    ArchivePreview {
        preview: Option<ArchivePreview>,
    },
    CommitMessage {
        message: String,
    },
    TerminalCommand {
        command: String,
    },
    /// `None` when `cwd` is not inside a Git repository.
    GitPanel {
        snapshot: Option<GitPanelSnapshot>,
    },
    Pull {
        outcome: PullOutcome,
    },
    Commits {
        entries: Vec<CommitEntry>,
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
    /// `repo` is `None` when `cwd` has no GitHub remote `gh` can resolve;
    /// `availability` says whether that is a `gh` problem or just "not
    /// GitHub".
    GitHubRepo {
        repo: Option<GitHubRepoRef>,
        availability: GitHubAvailability,
    },
    /// `None` when the host could not be read — same "unknown is not empty"
    /// contract as `PullRequests`.
    Issues {
        entries: Option<Vec<IssueSummary>>,
    },
    Issue {
        detail: Option<IssueDetail>,
    },
    PullRequest {
        detail: Option<PullRequestDetail>,
    },
    /// `None` when `cwd` is not inside a Git repository.
    RepoWorktrees {
        entries: Option<Vec<RepoWorktree>>,
    },
    /// `None` when `cwd` is not inside a Git repository.
    RepoBranches {
        entries: Option<Vec<RepoBranch>>,
    },
    /// One entry per branch Git refused to delete; an empty list means every
    /// requested branch is gone.
    BranchDeletions {
        failures: Vec<BranchDeleteFailure>,
    },
}
