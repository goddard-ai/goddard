//! Daemon-owned workspace filesystem and Git API.
//!
//! Paths in this module always name resources on the daemon host. A client
//! may display them, but must never reinterpret them against its own machine.

use std::collections::HashSet;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Output;

use anyhow::{Context as _, anyhow, bail};

const MAX_HYDRATED_PATCH_BYTES: usize = 32 * 1024 * 1024;
/// Binary reads base64-encode on the wire; 32 MiB stays well under
/// `MAX_WIRE_MESSAGE_BYTES` (48 MiB) once encoded.
const MAX_BINARY_FILE_BYTES: u64 = 32 * 1024 * 1024;
const EMPTY_TREE: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";

pub use waku_protocol::workspace::{
    ReclaimFailure, ReclaimablePath, ReviewDiffData, ReviewDiffSource, WorkingTreeEntry,
    WorkspaceOperation, WorkspaceResult,
};

/// `qa_branch` is the daemon's configured review-train branch — only the
/// `Review*` operations read it.
pub fn execute(operation: WorkspaceOperation, qa_branch: &str) -> anyhow::Result<WorkspaceResult> {
    Ok(match operation {
        WorkspaceOperation::ListTree {
            root,
            expanded_paths,
        } => WorkspaceResult::WorkingTree {
            entries: list_tree(&root, &expanded_paths.into_iter().collect()),
        },
        WorkspaceOperation::BrowseDirectory { path } => {
            let home = dirs::home_dir().ok_or_else(|| anyhow!("home directory is unavailable"))?;
            let path =
                dunce::canonicalize(path.as_deref().unwrap_or(&home)).with_context(|| {
                    format!(
                        "could not open directory {}",
                        path.as_deref().unwrap_or(&home).display()
                    )
                })?;
            if !fs::metadata(&path)?.is_dir() {
                bail!("not a directory: {}", path.display());
            }
            let filesystem_root = path
                .ancestors()
                .last()
                .map(Path::to_owned)
                .unwrap_or_else(|| path.clone());
            WorkspaceResult::Directory {
                parent: path.parent().map(Path::to_owned),
                entries: list_directory(&path)?,
                path,
                home,
                filesystem_root,
            }
        }
        WorkspaceOperation::ReadTextFile {
            root,
            relative_path,
        } => WorkspaceResult::TextFile {
            content: fs::read_to_string(resolve_workspace_path(&root, &relative_path)?)?,
        },
        WorkspaceOperation::ReadBinaryFile {
            root,
            relative_path,
        } => {
            let path = resolve_workspace_path(&root, &relative_path)?;
            let size = fs::metadata(&path)?.len();
            if size > MAX_BINARY_FILE_BYTES {
                bail!("file is too large to preview ({size} bytes, limit {MAX_BINARY_FILE_BYTES})");
            }
            let data = fs::read(&path)?;
            if data.len() as u64 > MAX_BINARY_FILE_BYTES {
                bail!("file grew past the preview size limit while reading");
            }
            WorkspaceResult::File { data }
        }
        WorkspaceOperation::WriteTextFile {
            root,
            relative_path,
            content,
        } => {
            fs::write(resolve_workspace_path(&root, &relative_path)?, content)?;
            WorkspaceResult::Ack
        }
        WorkspaceOperation::ListProjectFiles { root, cap } => WorkspaceResult::ProjectFiles {
            entries: crate::composer_complete::list_project_files(&root, cap),
        },
        WorkspaceOperation::SearchDirectories {
            roots,
            max_depth,
            cap,
        } => WorkspaceResult::Directories {
            paths: search_directories(&roots, max_depth, cap),
        },
        WorkspaceOperation::DiscoverSlashCommands {
            provider,
            project_root,
            binary_override,
        } => WorkspaceResult::SlashCommands {
            commands: crate::composer_complete::discover_slash_commands(
                provider,
                &project_root,
                binary_override.as_deref(),
            ),
        },
        WorkspaceOperation::CreateProjectlessWorkspace { prompt } => {
            WorkspaceResult::ProjectlessWorkspace {
                cwd: crate::projectless::create_workspace(prompt.as_deref())?.cwd,
            }
        }
        WorkspaceOperation::MigrateProjectlessWorkspace { path } => {
            WorkspaceResult::ProjectlessWorkspace {
                cwd: crate::projectless::migrate_workspace(&path)?.cwd,
            }
        }
        WorkspaceOperation::ArchiveProjectlessWorkspace { path } => {
            crate::projectless::archive_workspace(&path)?;
            WorkspaceResult::Ack
        }
        WorkspaceOperation::RestoreProjectlessWorkspace { path } => WorkspaceResult::Bool {
            value: crate::projectless::restore_workspace(&path)?,
        },
        WorkspaceOperation::DeleteProjectlessWorkspace { path } => {
            crate::projectless::remove_workspace(&path)?;
            WorkspaceResult::Ack
        }
        WorkspaceOperation::InspectBranches { cwd } => WorkspaceResult::Branches {
            snapshot: crate::git_branch::inspect(&cwd)?,
        },
        WorkspaceOperation::CheckoutBranch {
            cwd,
            branch,
            create,
        } => WorkspaceResult::BranchChanged {
            snapshot: if create {
                crate::git_branch::create_and_checkout(&cwd, &branch)?
            } else {
                crate::git_branch::checkout(&cwd, &branch)?
            },
        },
        WorkspaceOperation::ResetWorktree { path, base_ref } => WorkspaceResult::BranchChanged {
            snapshot: crate::git_branch::reset_to_base(&path, &base_ref)?,
        },
        WorkspaceOperation::CreateWorktree {
            project_path,
            name,
            base_ref,
            sync_default_branch,
            sync_branches,
        } => WorkspaceResult::WorktreeCreated {
            worktree: crate::worktree::create(
                &project_path,
                name.as_deref(),
                base_ref.as_deref(),
                sync_default_branch,
                &sync_branches,
            )?,
        },
        WorkspaceOperation::CreateWorktreeFromCheckout { project_path, name } => {
            WorkspaceResult::WorktreeCreated {
                worktree: crate::worktree::create_from_checkout(&project_path, name.as_deref())?,
            }
        }
        WorkspaceOperation::RemoveWorktree { path, force } => {
            crate::worktree::remove(&path, force)?;
            WorkspaceResult::Ack
        }
        WorkspaceOperation::EnsureWorktree {
            project_path,
            path,
            branch,
            base_ref,
        } => match crate::worktree::ensure(
            &project_path,
            &path,
            branch.as_deref(),
            base_ref.as_deref(),
        )? {
            Some(branch) => WorkspaceResult::WorktreeEnsured {
                created: true,
                branch,
            },
            None => WorkspaceResult::WorktreeEnsured {
                created: false,
                branch: None,
            },
        },
        WorkspaceOperation::ListWorktrees { cwd } => WorkspaceResult::RepoWorktrees {
            entries: crate::repo::list_worktrees(&cwd)?,
        },
        WorkspaceOperation::ListRepoBranches { cwd } => WorkspaceResult::RepoBranches {
            entries: crate::repo::list_repo_branches(&cwd)?,
        },
        WorkspaceOperation::FetchRemote { cwd, remote } => {
            crate::repo::fetch_remote(&cwd, &remote)?;
            WorkspaceResult::Ack
        }
        WorkspaceOperation::DeleteBranches { cwd, names, force } => {
            WorkspaceResult::BranchDeletions {
                failures: crate::repo::delete_branches(&cwd, &names, force)?,
            }
        }
        WorkspaceOperation::PruneWorktrees { cwd } => {
            crate::repo::prune_worktrees(&cwd)?;
            WorkspaceResult::Ack
        }
        WorkspaceOperation::RepairWorktrees { cwd, paths } => {
            crate::repo::repair_worktrees(&cwd, &paths)?;
            WorkspaceResult::Ack
        }
        WorkspaceOperation::InspectCommit { cwd } => WorkspaceResult::CommitSnapshot {
            snapshot: crate::git_commit::inspect(&cwd)?,
        },
        WorkspaceOperation::InspectCheckoutStatus { cwd } => WorkspaceResult::CheckoutStatus {
            status: crate::git_commit::checkout_status(&cwd)?,
        },
        WorkspaceOperation::InspectArchivePreview { cwd } => WorkspaceResult::ArchivePreview {
            preview: crate::git_commit::archive_preview(&cwd)?,
        },
        WorkspaceOperation::InspectReclaimable { cwd } => WorkspaceResult::Reclaimable {
            entries: inspect_reclaimable(&cwd),
        },
        WorkspaceOperation::ReclaimPaths { cwd, paths } => {
            let (reclaimed_bytes, failures) = reclaim_paths(&cwd, &paths);
            WorkspaceResult::Reclaim {
                reclaimed_bytes,
                failures,
            }
        }
        WorkspaceOperation::GenerateCommitMessage {
            cwd,
            include_unstaged,
            invocation,
        } => WorkspaceResult::CommitMessage {
            message: crate::git_commit::generate_message(&cwd, include_unstaged, &invocation)?,
        },
        WorkspaceOperation::GenerateTerminalCommand {
            cwd,
            request,
            scrollback,
            shell,
            invocation,
        } => WorkspaceResult::TerminalCommand {
            command: crate::shell_command::generate_command(
                &cwd,
                &request,
                scrollback.as_deref(),
                shell.as_deref(),
                &invocation,
            )?,
        },
        WorkspaceOperation::Commit {
            cwd,
            message,
            include_unstaged,
            push,
        } => {
            crate::git_commit::commit(&cwd, &message, include_unstaged)?;
            if push {
                crate::git_commit::push(&cwd)?;
            }
            WorkspaceResult::Ack
        }
        WorkspaceOperation::Push { cwd } => {
            crate::git_commit::push(&cwd)?;
            WorkspaceResult::Ack
        }
        WorkspaceOperation::PushBase { cwd, base } => WorkspaceResult::PushBase {
            outcome: crate::git_panel::push_base(&cwd, &base)?,
        },
        WorkspaceOperation::SyncBase {
            cwd,
            base,
            strategy,
        } => {
            let (checkout, outcome) = crate::git_panel::sync_base(&cwd, &base, strategy)?;
            WorkspaceResult::SyncBase { checkout, outcome }
        }
        WorkspaceOperation::BasePushState { cwd, base } => WorkspaceResult::BasePushState {
            state: crate::git_panel::base_push_state(&cwd, &base)?,
        },
        WorkspaceOperation::FetchUpstream { cwd, base } => WorkspaceResult::Bool {
            value: crate::git_panel::fetch_upstream(&cwd, &base)?,
        },
        WorkspaceOperation::InspectGitPanel { cwd, base } => WorkspaceResult::GitPanel {
            snapshot: crate::git_panel::inspect(&cwd, base.as_deref())?,
        },
        WorkspaceOperation::StageFile { cwd, path } => {
            crate::git_panel::stage(&cwd, &path)?;
            WorkspaceResult::Ack
        }
        WorkspaceOperation::UnstageFile { cwd, path } => {
            crate::git_panel::unstage(&cwd, &path)?;
            WorkspaceResult::Ack
        }
        WorkspaceOperation::DiscardFile { cwd, path } => {
            crate::git_panel::discard(&cwd, &path)?;
            WorkspaceResult::Ack
        }
        WorkspaceOperation::ReadFileAtRef { cwd, path, git_ref } => WorkspaceResult::TextFile {
            content: crate::git_panel::file_at_ref(&cwd, &path, &git_ref)?,
        },
        WorkspaceOperation::IgnoreFile { cwd, path } => {
            crate::git_panel::ignore(&cwd, &path)?;
            WorkspaceResult::Ack
        }
        WorkspaceOperation::PullUpstream { cwd, strategy } => WorkspaceResult::Pull {
            outcome: crate::git_panel::pull(&cwd, strategy)?,
        },
        WorkspaceOperation::AbortSync { cwd } => {
            crate::git_panel::abort_sync(&cwd)?;
            WorkspaceResult::Ack
        }
        WorkspaceOperation::Land {
            cwd,
            base,
            strategy,
        } => WorkspaceResult::Land {
            outcome: crate::git_panel::land(&cwd, base.as_deref(), strategy)?,
        },
        WorkspaceOperation::RebaseOnto {
            cwd,
            base,
            onto,
            strategy,
        } => WorkspaceResult::Rebase {
            outcome: crate::git_panel::rebase_onto(&cwd, &base, onto.as_deref(), strategy)?,
        },
        WorkspaceOperation::ListCommits { cwd, skip, limit } => WorkspaceResult::Commits {
            entries: crate::git_panel::commits(&cwd, skip, limit)?,
        },
        WorkspaceOperation::ListUpstreamCommits { cwd, skip, limit } => WorkspaceResult::Commits {
            entries: crate::git_panel::upstream_commits(&cwd, skip, limit)?,
        },
        WorkspaceOperation::ResolveRemoteFile { cwd, path } => WorkspaceResult::RemoteFile {
            file: crate::git_branch::remote_file(&cwd, &path)?,
        },
        WorkspaceOperation::FileDiff { cwd, path, staged } => WorkspaceResult::ReviewDiff {
            data: file_diff(&cwd, &path, staged)?,
        },
        WorkspaceOperation::CommitDiff { cwd, sha } => WorkspaceResult::ReviewDiff {
            data: commit_diff(&cwd, &sha)?,
        },
        WorkspaceOperation::CommitEntry { cwd, sha } => WorkspaceResult::CommitEntry {
            entry: crate::git_panel::commit(&cwd, &sha)?,
        },
        WorkspaceOperation::ReviewQueue { cwd } => WorkspaceResult::ReviewQueue {
            queue: crate::review::queue(&cwd, qa_branch)?,
        },
        WorkspaceOperation::ReviewApprove { cwd, sha } => WorkspaceResult::ReviewQueue {
            queue: crate::review::approve(&cwd, &sha, qa_branch)?,
        },
        WorkspaceOperation::ReviewReject { cwd, sha } => WorkspaceResult::ReviewQueue {
            queue: crate::review::reject(&cwd, &sha, qa_branch)?,
        },
        WorkspaceOperation::ReviewPromote { cwd } => WorkspaceResult::ReviewQueue {
            queue: crate::review::promote(&cwd, qa_branch)?,
        },
        WorkspaceOperation::CaptureTurnStart {
            cwd,
            session_id,
            turn_count,
        } => {
            crate::checkpoint::capture_turn_start(&cwd, session_id, turn_count)?;
            WorkspaceResult::Ack
        }
        WorkspaceOperation::CaptureTurn {
            cwd,
            session_id,
            turn_count,
        } => WorkspaceResult::Checkpoint {
            checkpoint: crate::checkpoint::capture_turn(&cwd, session_id, turn_count)?,
        },
        WorkspaceOperation::CaptureRef { cwd, git_ref } => {
            crate::checkpoint::capture_ref(&cwd, &git_ref)?;
            WorkspaceResult::Ack
        }
        WorkspaceOperation::RestoreRef { cwd, git_ref } => {
            crate::checkpoint::restore_ref(&cwd, &git_ref)?;
            WorkspaceResult::Ack
        }
        WorkspaceOperation::HasRef { cwd, git_ref } => WorkspaceResult::Bool {
            value: crate::checkpoint::has_ref(&cwd, &git_ref),
        },
        WorkspaceOperation::SessionTurnRefs { cwd, session_id } => {
            let mut turn_counts = crate::checkpoint::session_turn_refs(&cwd, session_id)
                .into_iter()
                .collect::<Vec<_>>();
            turn_counts.sort_unstable();
            WorkspaceResult::TurnRefs { turn_counts }
        }
        WorkspaceOperation::DeleteRef { cwd, git_ref } => {
            crate::checkpoint::delete_ref(&cwd, &git_ref)?;
            WorkspaceResult::Ack
        }
        WorkspaceOperation::DeleteTurnRefsAfter {
            cwd,
            session_id,
            retained_turn_count,
            previous_turn_count,
        } => {
            crate::checkpoint::delete_turn_refs_after(
                &cwd,
                session_id,
                retained_turn_count,
                previous_turn_count,
            )?;
            WorkspaceResult::Ack
        }
        WorkspaceOperation::DeleteSessionRefs { cwd, session_id } => {
            crate::checkpoint::delete_all_session_refs(&cwd, session_id)?;
            WorkspaceResult::Ack
        }
        WorkspaceOperation::CopySessionRefs {
            cwd,
            source_session_id,
            target_session_id,
            through_turn_count,
        } => {
            crate::checkpoint::copy_session_refs(
                &cwd,
                source_session_id,
                target_session_id,
                through_turn_count,
            )?;
            WorkspaceResult::Ack
        }
        WorkspaceOperation::ListPullRequests { cwd, head_branch } => {
            WorkspaceResult::PullRequests {
                entries: crate::pull_requests::list(&cwd, &head_branch)?,
            }
        }
        WorkspaceOperation::ResolveGitHubRepo { cwd } => {
            let (repo, availability) = crate::github::resolve_repo(&cwd);
            WorkspaceResult::GitHubRepo { repo, availability }
        }
        WorkspaceOperation::ListGitHubActivity { cwd } => WorkspaceResult::GitHubActivity {
            releases: crate::github::list_releases(&cwd)?,
            runs: crate::github::list_workflow_runs(&cwd)?,
        },
        WorkspaceOperation::GetGitHubRelease { cwd, tag } => WorkspaceResult::GitHubRelease {
            detail: crate::github::release(&cwd, &tag)?,
        },
        WorkspaceOperation::GetGitHubWorkflowRun { cwd, run_id } => {
            WorkspaceResult::GitHubWorkflowRun {
                detail: crate::github::workflow_run(&cwd, run_id)?,
            }
        }
        WorkspaceOperation::ListIssues { cwd, state, query } => WorkspaceResult::Issues {
            entries: crate::issues::list(&cwd, state, query.as_deref())?,
        },
        WorkspaceOperation::GetIssue { cwd, number } => WorkspaceResult::Issue {
            detail: crate::issues::view(&cwd, number)?,
        },
        WorkspaceOperation::ListIssueTemplates { cwd } => {
            let (entries, blank_issues_enabled) = crate::issue_templates::list(&cwd);
            WorkspaceResult::IssueTemplates {
                entries,
                blank_issues_enabled,
            }
        }
        WorkspaceOperation::CreateIssue { cwd, input } => {
            let (number, url) = crate::issues::create(&cwd, &input)?;
            WorkspaceResult::IssueCreated { number, url }
        }
        WorkspaceOperation::ListRepoPullRequests { cwd, state, query } => {
            WorkspaceResult::PullRequests {
                entries: crate::pull_requests::list_for_repo(&cwd, state, query.as_deref())?,
            }
        }
        WorkspaceOperation::GetPullRequest { cwd, number } => WorkspaceResult::PullRequest {
            detail: crate::pull_requests::view(&cwd, number)?,
        },
        WorkspaceOperation::PostWorkItemComment {
            cwd,
            kind,
            number,
            body,
        } => {
            match kind {
                waku_protocol::workspace::WorkItemKind::PullRequest => {
                    crate::pull_requests::comment(&cwd, number, &body)?
                }
                waku_protocol::workspace::WorkItemKind::Issue => {
                    crate::issues::comment(&cwd, number, &body)?
                }
            }
            WorkspaceResult::Ack
        }
        WorkspaceOperation::FetchPullRequestHead {
            cwd,
            number,
            branch,
        } => {
            crate::pull_requests::fetch_head(&cwd, number, &branch)?;
            WorkspaceResult::Ack
        }
        WorkspaceOperation::ListNotifications {
            all,
            etag,
            if_modified_since,
        } => WorkspaceResult::Notifications {
            poll: crate::notifications::list(etag.as_deref(), if_modified_since.as_deref(), all)?,
        },
        WorkspaceOperation::MarkNotificationRead { thread_id } => {
            crate::notifications::mark_read(&thread_id)?;
            WorkspaceResult::Ack
        }
        WorkspaceOperation::MarkNotificationDone { thread_id } => {
            crate::notifications::mark_done(&thread_id)?;
            WorkspaceResult::Ack
        }
        WorkspaceOperation::MarkRepoNotificationsRead { repo } => {
            crate::notifications::mark_repo_read(&repo)?;
            WorkspaceResult::Ack
        }
        WorkspaceOperation::MarkAllNotificationsRead => {
            crate::notifications::mark_all_read()?;
            WorkspaceResult::Ack
        }
        WorkspaceOperation::CollectReviewDiff { cwd, source } => WorkspaceResult::ReviewDiff {
            data: collect_review_diff(&cwd, source)?,
        },
    })
}

fn resolve_workspace_path(root: &Path, relative: &Path) -> anyhow::Result<PathBuf> {
    if relative.as_os_str().is_empty()
        || relative.is_absolute()
        || relative
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        bail!("workspace path must be a non-empty relative path");
    }
    Ok(root.join(relative))
}

fn list_tree(root: &Path, expanded_paths: &HashSet<PathBuf>) -> Vec<WorkingTreeEntry> {
    fn visit(
        directory: &Path,
        relative_directory: &Path,
        depth: usize,
        expanded_paths: &HashSet<PathBuf>,
        output: &mut Vec<WorkingTreeEntry>,
    ) {
        let Ok(entries) = fs::read_dir(directory) else {
            return;
        };
        let mut children = entries
            .filter_map(Result::ok)
            .filter_map(|entry| {
                let name = entry.file_name().to_string_lossy().into_owned();
                if name == ".git" {
                    return None;
                }
                let is_dir = entry.file_type().ok()?.is_dir();
                Some((entry.path(), name, is_dir))
            })
            .collect::<Vec<_>>();
        children.sort_by_key(|(_, name, is_dir)| (!*is_dir, name.to_lowercase()));
        for (absolute_path, name, is_dir) in children {
            let relative_path = relative_directory.join(&name);
            let expanded = is_dir && expanded_paths.contains(&absolute_path);
            output.push(WorkingTreeEntry {
                relative_path: relative_path.to_string_lossy().into_owned(),
                absolute_path: absolute_path.clone(),
                name,
                is_dir,
                expanded,
                depth,
            });
            if expanded {
                visit(
                    &absolute_path,
                    &relative_path,
                    depth + 1,
                    expanded_paths,
                    output,
                );
            }
        }
    }
    let mut output = Vec::new();
    visit(root, Path::new(""), 0, expanded_paths, &mut output);
    output
}

fn list_directory(directory: &Path) -> anyhow::Result<Vec<WorkingTreeEntry>> {
    let mut entries = fs::read_dir(directory)
        .with_context(|| format!("could not read directory {}", directory.display()))?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let is_dir = fs::metadata(entry.path()).ok()?.is_dir();
            Some(WorkingTreeEntry {
                relative_path: name.clone(),
                absolute_path: entry.path(),
                name,
                is_dir,
                expanded: false,
                depth: 0,
            })
        })
        .collect::<Vec<_>>();
    entries.sort_by_key(|entry| (!entry.is_dir, entry.name.to_lowercase()));
    Ok(entries)
}

/// The directories under `cwd` that are safe to delete for space:
/// git-ignored — untracked, so never source — and named on the
/// reproducible-output allowlist. `git ls-files` collapses each ignored
/// directory to one `name/` entry at every level, which is how nested
/// `node_modules` roots surface. Anything but a successful listing is an
/// empty set — a non-checkout simply has nothing to reclaim.
fn reclaimable_dirs(cwd: &Path) -> Vec<PathBuf> {
    let output = crate::command_env::search_path_command("git")
        .args([
            "ls-files",
            "--others",
            "--ignored",
            "--exclude-standard",
            "--directory",
            "-z",
        ])
        .current_dir(cwd)
        .output();
    let Ok(output) = output else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    output
        .stdout
        .split(|byte| *byte == 0)
        .filter_map(|entry| {
            // Only directories carry the trailing slash; ignored files
            // never qualify.
            let entry = std::str::from_utf8(entry).ok()?.strip_suffix('/')?;
            let name = Path::new(entry).file_name()?.to_str()?;
            RECLAIMABLE_DIR_NAMES
                .contains(&name)
                .then(|| cwd.join(entry))
        })
        .collect()
}

/// One directory's on-disk size. `DirEntry::metadata` doesn't follow
/// symlinks, so a linked subtree is charged once — the link's own bytes —
/// rather than walked at its target.
fn directory_size(path: &Path) -> u64 {
    let mut total = 0u64;
    let mut pending = vec![path.to_path_buf()];
    while let Some(directory) = pending.pop() {
        let Ok(entries) = fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(metadata) = entry.metadata() else {
                continue;
            };
            if metadata.is_dir() {
                pending.push(entry.path());
            } else {
                total += metadata.len();
            }
        }
    }
    total
}

fn inspect_reclaimable(cwd: &Path) -> Vec<ReclaimablePath> {
    reclaimable_dirs(cwd)
        .into_iter()
        .map(|path| ReclaimablePath {
            bytes: directory_size(&path),
            path,
        })
        .collect()
}

/// Delete the reported paths still qualifying under a fresh scan — the
/// client names the set, the daemon re-derives the safe one, and only
/// the intersection goes. Returns the freed bytes with one failure per
/// path the delete could not remove.
fn reclaim_paths(cwd: &Path, paths: &[PathBuf]) -> (u64, Vec<ReclaimFailure>) {
    let allowed: HashSet<PathBuf> = reclaimable_dirs(cwd).into_iter().collect();
    let mut reclaimed_bytes = 0u64;
    let mut failures = Vec::new();
    for path in paths {
        if !allowed.contains(path) {
            continue;
        }
        // A symlink reports `is_dir` false under symlink_metadata and is
        // skipped — `remove_dir_all` never follows it off the worktree.
        if !fs::symlink_metadata(path).is_ok_and(|meta| meta.is_dir()) {
            continue;
        }
        let bytes = directory_size(path);
        match fs::remove_dir_all(path) {
            Ok(()) => reclaimed_bytes += bytes,
            Err(error) => failures.push(ReclaimFailure {
                path: path.clone(),
                error: error.to_string(),
            }),
        }
    }
    (reclaimed_bytes, failures)
}

/// Directory names whose contents a toolchain reproduces — dependency
/// installs and build output — so deleting one costs a reinstall or a
/// rebuild, never work. The reclaim scan intersects this list with the
/// checkout's git-ignored directories, which is the real safety hinge:
/// a tracked directory with one of these names can never be reported.
const RECLAIMABLE_DIR_NAMES: [&str; 19] = [
    "node_modules",
    "target",
    ".expo",
    ".next",
    ".turbo",
    "dist",
    "build",
    "out",
    "coverage",
    ".venv",
    "venv",
    "__pycache__",
    "Pods",
    "DerivedData",
    ".build",
    ".gradle",
    ".goddard-cache",
    ".waku-cache",
    "graft",
];

/// Directory names a picker walk never descends into, beyond the hidden
/// `.`-prefixed trees: dependency and build output (the same exclusions as
/// the composer file walk) plus the platform home directories that are
/// effectively never project roots.
const DIRECTORY_SEARCH_SKIP: [&str; 13] = [
    "node_modules",
    "target",
    "dist",
    "build",
    "out",
    "vendor",
    "__pycache__",
    "Library",
    "Applications",
    "Movies",
    "Music",
    "Pictures",
    "Public",
];

/// Directories under `roots` a picker can offer. A root nested inside
/// another contributes nothing new and is skipped. Results carry their
/// depth and repo-ness so the caller's ordering — repositories first, then
/// shallow paths — needs no second filesystem pass; `max_depth` bounds both
/// the work and any symlink cycle, `cap` the result size.
fn search_directories(roots: &[PathBuf], max_depth: usize, cap: usize) -> Vec<PathBuf> {
    fn visit(
        directory: &Path,
        depth: usize,
        max_depth: usize,
        found: &mut Vec<(bool, usize, PathBuf)>,
        cap: usize,
    ) {
        if depth > max_depth || found.len() >= cap {
            return;
        }
        let Ok(entries) = fs::read_dir(directory) else {
            return;
        };
        for entry in entries.flatten() {
            if found.len() >= cap {
                break;
            }
            let file_name = entry.file_name();
            let Some(name) = file_name.to_str() else {
                continue;
            };
            if name.starts_with('.') || DIRECTORY_SEARCH_SKIP.contains(&name) {
                continue;
            }
            // `fs::metadata` follows symlinks; the depth cap bounds the
            // cycles a linked tree could create.
            let Ok(metadata) = fs::metadata(entry.path()) else {
                continue;
            };
            if !metadata.is_dir() {
                continue;
            }
            let path = entry.path();
            let is_repo = path.join(".git").exists();
            found.push((is_repo, depth, path.clone()));
            visit(&path, depth + 1, max_depth, found, cap);
        }
    }

    let mut roots = roots.to_vec();
    roots.sort();
    roots.dedup();
    let mut walked: Vec<PathBuf> = Vec::new();
    let mut found: Vec<(bool, usize, PathBuf)> = Vec::new();
    for root in roots {
        if found.len() >= cap {
            break;
        }
        // Overlapping roots in a filesystem tree always nest, so a prior
        // root containing this one would list the same directories twice.
        if walked.iter().any(|covered| root.starts_with(covered)) {
            continue;
        }
        if fs::metadata(&root).is_ok_and(|metadata| metadata.is_dir()) {
            visit(&root, 1, max_depth, &mut found, cap);
            walked.push(root);
        }
    }
    found.sort_by(|a, b| (!a.0, a.1, &a.2).cmp(&(!b.0, b.1, &b.2)));
    found.truncate(cap);
    found.into_iter().map(|(_, _, path)| path).collect()
}

#[derive(Clone, Debug)]
struct DiffRange {
    from: String,
    to: String,
}

fn collect_review_diff(cwd: &Path, source: ReviewDiffSource) -> anyhow::Result<ReviewDiffData> {
    ensure_repository(cwd)?;
    let range = resolve_diff_range(cwd, source)?;
    let numstat = diff_output(cwd, &range, &["--numstat"])?;
    let hydrated = diff_output(cwd, &range, &["--unified=2147483647"])?;
    let (patch, complete_context) = if hydrated.len() <= MAX_HYDRATED_PATCH_BYTES {
        (hydrated, true)
    } else {
        (diff_output(cwd, &range, &["--unified=3"])?, false)
    };
    Ok(ReviewDiffData {
        source,
        numstat,
        patch,
        complete_context,
    })
}

/// The Git panel's per-file hover preview: the file's staged (`--cached`) or
/// unstaged diff, capped the same way `collect_review_diff` caps its patch.
/// An unstaged path with no index entry is an untracked file and diffs as a
/// new file through `--no-index`.
fn file_diff(cwd: &Path, path: &str, staged: bool) -> anyhow::Result<ReviewDiffData> {
    ensure_repository(cwd)?;
    let source = if staged {
        ReviewDiffSource::Staged
    } else {
        ReviewDiffSource::Unstaged
    };
    let tracked = staged || !git(cwd, ["ls-files", "-z", "--", path])?.is_empty();
    // Tracked: `git diff [--cached] -- path`. Untracked: the file versus
    // `/dev/null`, which `diff --no-index` reports as a new-file patch.
    let diff_args = |mode: &'static str| -> Vec<&str> {
        if tracked {
            let mut args = vec![mode];
            if staged {
                args.push("--cached");
            }
            args.extend(["--", path]);
            args
        } else {
            vec![mode, "--no-index", "/dev/null", path]
        }
    };
    let numstat = file_diff_output(cwd, &diff_args("--numstat"), !tracked)?;
    let hydrated = file_diff_output(cwd, &diff_args("--unified=2147483647"), !tracked)?;
    let (patch, complete_context) = if hydrated.len() <= MAX_HYDRATED_PATCH_BYTES {
        (hydrated, true)
    } else {
        (
            file_diff_output(cwd, &diff_args("--unified=3"), !tracked)?,
            false,
        )
    };
    Ok(ReviewDiffData {
        source,
        numstat,
        patch,
        complete_context,
    })
}

/// One commit's diff for the Git panel's modal: the commit against its first
/// parent, or the empty tree for a root commit. A merge reads as the delta
/// the merge brought in.
fn commit_diff(cwd: &Path, sha: &str) -> anyhow::Result<ReviewDiffData> {
    ensure_repository(cwd)?;
    let to = resolve(cwd, sha).ok_or_else(|| anyhow!("unknown commit {sha}"))?;
    let from = resolve(cwd, &format!("{sha}^")).unwrap_or_else(|| EMPTY_TREE.to_owned());
    let range = DiffRange { from, to };
    let numstat = diff_output(cwd, &range, &["--numstat"])?;
    let hydrated = diff_output(cwd, &range, &["--unified=2147483647"])?;
    let (patch, complete_context) = if hydrated.len() <= MAX_HYDRATED_PATCH_BYTES {
        (hydrated, true)
    } else {
        (diff_output(cwd, &range, &["--unified=3"])?, false)
    };
    Ok(ReviewDiffData {
        source: ReviewDiffSource::Commit,
        numstat,
        patch,
        complete_context,
    })
}

/// `git diff` output for a single path. `no_index` runs the
/// `/dev/null`-vs-file form, where exit status 1 still means success (the
/// files differed).
fn file_diff_output(cwd: &Path, args: &[&str], no_index: bool) -> anyhow::Result<String> {
    let output = crate::command_env::search_path_command("git")
        .args([
            "-c",
            "core.quotePath=false",
            "diff",
            "--no-ext-diff",
            "--no-color",
        ])
        .args(args)
        .current_dir(cwd)
        .output()
        .context("failed to generate Git diff")?;
    if output.status.success() || (no_index && output.status.code() == Some(1)) {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        bail!("{}", command_error(&output))
    }
}

fn resolve_diff_range(cwd: &Path, source: ReviewDiffSource) -> anyhow::Result<DiffRange> {
    let head = resolve(cwd, "HEAD").unwrap_or_else(|| EMPTY_TREE.to_owned());
    Ok(match source {
        ReviewDiffSource::LastTurn {
            session_id,
            turn_count,
            ..
        } => {
            if turn_count == 0 {
                bail!("the first checkpoint is a baseline, not a completed turn");
            }
            let diff_base_ref = crate::checkpoint::turn_diff_base_ref(session_id, turn_count);
            let start_ref = crate::checkpoint::turn_start_ref(session_id, turn_count);
            let legacy_ref = crate::checkpoint::checkpoint_ref(session_id, turn_count - 1);
            let to_ref = crate::checkpoint::checkpoint_ref(session_id, turn_count);
            DiffRange {
                from: resolve(cwd, &diff_base_ref)
                    .or_else(|| resolve(cwd, &start_ref))
                    .or_else(|| resolve(cwd, &legacy_ref))
                    .ok_or_else(|| anyhow!("the turn's starting checkpoint is unavailable"))?,
                to: resolve(cwd, &to_ref)
                    .ok_or_else(|| anyhow!("the turn's ending checkpoint is unavailable"))?,
            }
        }
        ReviewDiffSource::Uncommitted => DiffRange {
            from: head,
            to: crate::checkpoint::capture_worktree_commit(cwd)?,
        },
        ReviewDiffSource::Unstaged => DiffRange {
            from: index_tree(cwd)?,
            to: crate::checkpoint::capture_worktree_commit(cwd)?,
        },
        ReviewDiffSource::Staged => DiffRange {
            from: head,
            to: index_tree(cwd)?,
        },
        ReviewDiffSource::Committed => DiffRange {
            from: branch_base(cwd)?,
            to: head,
        },
        ReviewDiffSource::Branch => DiffRange {
            from: branch_base(cwd)?,
            to: crate::checkpoint::capture_worktree_commit(cwd)?,
        },
        ReviewDiffSource::Commit => {
            bail!("commit diffs are collected by the CommitDiff operation")
        }
    })
}

fn branch_base(cwd: &Path) -> anyhow::Result<String> {
    let Some(snapshot) = crate::git_branch::inspect(cwd)? else {
        bail!("the workspace is not a Git repository");
    };
    let Some(head) = resolve(cwd, "HEAD") else {
        return Ok(EMPTY_TREE.to_owned());
    };
    let current = snapshot.current.as_deref();
    let default_branch = snapshot
        .default_branch
        .filter(|branch| current != Some(branch.as_str()))
        .or_else(|| {
            ["main", "master"]
                .into_iter()
                .find(|candidate| {
                    current != Some(*candidate)
                        && snapshot
                            .branches
                            .iter()
                            .any(|branch| branch.name == *candidate)
                })
                .map(str::to_owned)
        });
    let Some(default_branch) = default_branch else {
        return Ok(head);
    };
    let output = git(cwd, ["merge-base", "HEAD", default_branch.as_str()])?;
    let base = output.trim();
    Ok(if base.is_empty() {
        head
    } else {
        base.to_owned()
    })
}

fn index_tree(cwd: &Path) -> anyhow::Result<String> {
    let output = crate::command_env::search_path_command("git")
        .args(["write-tree"])
        .current_dir(cwd)
        .output()
        .context("failed to snapshot the Git index")?;
    if output.status.success() {
        let tree = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        if !tree.is_empty() {
            return Ok(tree);
        }
    }
    if resolve(cwd, "HEAD").is_none() {
        Ok(EMPTY_TREE.to_owned())
    } else {
        bail!("{}", command_error(&output))
    }
}

fn diff_output(cwd: &Path, range: &DiffRange, modes: &[&str]) -> anyhow::Result<String> {
    let output = crate::command_env::search_path_command("git")
        .args([
            "-c",
            "core.quotePath=false",
            "diff",
            "--no-ext-diff",
            "--no-color",
        ])
        .args(modes)
        .arg("--no-renames")
        .arg(&range.from)
        .arg(&range.to)
        .args(["--", "."])
        .current_dir(cwd)
        .output()
        .context("failed to generate Git diff")?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        bail!("{}", command_error(&output))
    }
}

fn ensure_repository(cwd: &Path) -> anyhow::Result<()> {
    let output = crate::command_env::search_path_command("git")
        .args(["rev-parse", "--is-inside-work-tree"])
        .current_dir(cwd)
        .output()
        .context("failed to inspect Git workspace")?;
    if output.status.success() {
        Ok(())
    } else {
        bail!("the workspace is not a Git repository")
    }
}

fn resolve(cwd: &Path, revision: &str) -> Option<String> {
    let output = crate::command_env::search_path_command("git")
        .args(["rev-parse", "--verify", &format!("{revision}^{{commit}}")])
        .current_dir(cwd)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn git<I, S>(cwd: &Path, args: I) -> anyhow::Result<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
{
    let output = crate::command_env::search_path_command("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .context("failed to execute git")?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        bail!("{}", command_error(&output))
    }
}

fn command_error(output: &Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    if stderr.is_empty() {
        format!("git exited with {}", output.status)
    } else {
        stderr
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use uuid::Uuid;

    fn git_ok(cwd: &Path, args: &[&str]) {
        let output = crate::command_env::search_path_command("git")
            .args(args)
            .current_dir(cwd)
            .output()
            .unwrap();
        assert!(output.status.success(), "{}", command_error(&output));
    }

    fn repository() -> PathBuf {
        let root = std::env::temp_dir().join(format!("waku-workspace-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        git_ok(&root, &["init", "-b", "main"]);
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/lib.rs"), "fn baseline() {}\n").unwrap();
        git_ok(&root, &["add", "."]);
        git_ok(
            &root,
            &[
                "-c",
                "user.name=Goddard Tests",
                "-c",
                "user.email=waku@example.com",
                "commit",
                "-m",
                "baseline",
            ],
        );
        root
    }

    fn collect(root: &Path, source: ReviewDiffSource) -> ReviewDiffData {
        let WorkspaceResult::ReviewDiff { data } = execute(
            WorkspaceOperation::CollectReviewDiff {
                cwd: root.to_path_buf(),
                source,
            },
            "qa",
        )
        .unwrap() else {
            panic!("unexpected workspace response")
        };
        data
    }

    fn numstat_summary(numstat: &str) -> (usize, u64, u64) {
        let mut files = 0;
        let mut additions = 0;
        let mut deletions = 0;
        for line in numstat.lines().filter(|line| !line.is_empty()) {
            let mut fields = line.splitn(3, '\t');
            additions += fields
                .next()
                .and_then(|value| value.parse().ok())
                .unwrap_or(0);
            deletions += fields
                .next()
                .and_then(|value| value.parse().ok())
                .unwrap_or(0);
            assert!(fields.next().is_some(), "numstat row is missing its path");
            files += 1;
        }
        (files, additions, deletions)
    }

    #[test]
    fn directory_browser_lists_an_arbitrary_daemon_directory() {
        let directory =
            std::env::temp_dir().join(format!("waku-directory-browser-{}", Uuid::new_v4()));
        fs::create_dir_all(directory.join("folder")).unwrap();
        fs::create_dir_all(directory.join(".git")).unwrap();
        fs::write(directory.join("notes.txt"), "notes").unwrap();

        let WorkspaceResult::Directory {
            path,
            parent,
            entries,
            ..
        } = execute(
            WorkspaceOperation::BrowseDirectory {
                path: Some(directory.clone()),
            },
            "qa",
        )
        .unwrap()
        else {
            panic!("unexpected workspace response")
        };

        assert_eq!(path, dunce::canonicalize(&directory).unwrap());
        assert_eq!(parent, path.parent().map(Path::to_owned));
        assert_eq!(
            entries
                .iter()
                .map(|entry| (&*entry.name, entry.is_dir))
                .collect::<Vec<_>>(),
            [(".git", true), ("folder", true), ("notes.txt", false)]
        );

        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn file_reader_keeps_the_complete_disk_content() {
        let root = std::env::temp_dir().join(format!("waku-editor-file-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let content = "line\n".repeat(10_000);
        fs::write(root.join("large.txt"), &content).unwrap();

        let WorkspaceResult::TextFile { content: restored } = execute(
            WorkspaceOperation::ReadTextFile {
                root: root.clone(),
                relative_path: PathBuf::from("large.txt"),
            },
            "qa",
        )
        .unwrap() else {
            panic!("unexpected workspace response")
        };
        assert_eq!(restored, content);
        assert!(
            execute(
                WorkspaceOperation::ReadTextFile {
                    root: root.clone(),
                    relative_path: PathBuf::from("missing.txt"),
                },
                "qa",
            )
            .is_err()
        );

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn source_modes_compare_consistent_git_snapshots() {
        let root = repository();
        git_ok(&root, &["switch", "-c", "feature"]);
        fs::write(root.join("src/lib.rs"), "fn committed() {}\n").unwrap();
        git_ok(&root, &["add", "src/lib.rs"]);
        git_ok(
            &root,
            &[
                "-c",
                "user.name=Goddard Tests",
                "-c",
                "user.email=waku@example.com",
                "commit",
                "-m",
                "feature",
            ],
        );
        fs::write(
            root.join("src/lib.rs"),
            "fn committed() {}\nfn staged() {}\n",
        )
        .unwrap();
        git_ok(&root, &["add", "src/lib.rs"]);
        fs::write(
            root.join("src/lib.rs"),
            "fn committed() {}\nfn staged() {}\nfn unstaged() {}\n",
        )
        .unwrap();
        fs::write(root.join("new file.txt"), "untracked\n").unwrap();

        let committed = collect(&root, ReviewDiffSource::Committed);
        let staged = collect(&root, ReviewDiffSource::Staged);
        let unstaged = collect(&root, ReviewDiffSource::Unstaged);
        let uncommitted = collect(&root, ReviewDiffSource::Uncommitted);
        let branch = collect(&root, ReviewDiffSource::Branch);

        assert_eq!(numstat_summary(&committed.numstat), (1, 1, 1));
        assert_eq!(numstat_summary(&staged.numstat), (1, 1, 0));
        assert_eq!(
            numstat_summary(&unstaged.numstat),
            (2, 2, 0),
            "unstaged includes untracked files"
        );
        assert_eq!(numstat_summary(&uncommitted.numstat), (2, 3, 0));
        assert_eq!(numstat_summary(&branch.numstat), (2, 4, 1));
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn last_turn_uses_captured_checkpoints_not_the_live_worktree() {
        let root = repository();
        let session_id = Uuid::new_v4();
        crate::checkpoint::capture_turn(&root, session_id, 0).unwrap();
        fs::write(
            root.join("src/lib.rs"),
            "fn baseline() {}\nfn from_turn() {}\n",
        )
        .unwrap();
        crate::checkpoint::capture_turn(&root, session_id, 1).unwrap();
        fs::write(
            root.join("src/lib.rs"),
            "fn baseline() {}\nfn from_turn() {}\nfn after_turn() {}\n",
        )
        .unwrap();

        let data = collect(
            &root,
            ReviewDiffSource::LastTurn {
                session_id,
                turn_id: Uuid::new_v4(),
                turn_count: 1,
            },
        );
        assert_eq!(numstat_summary(&data.numstat), (1, 1, 0));
        assert!(data.patch.contains("from_turn"));
        assert!(!data.patch.contains("after_turn"));
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn last_turn_review_uses_the_branch_aware_diff_base() {
        let root = repository();
        git_ok(&root, &["switch", "-c", "feature"]);
        fs::write(root.join("feature-only.rs"), "fn feature() {}\n").unwrap();
        git_ok(&root, &["add", "feature-only.rs"]);
        git_ok(
            &root,
            &[
                "-c",
                "user.name=Goddard Tests",
                "-c",
                "user.email=waku@example.com",
                "commit",
                "-m",
                "feature baseline",
            ],
        );
        git_ok(&root, &["switch", "main"]);
        fs::write(root.join("main-only.rs"), "fn main_only() {}\n").unwrap();
        git_ok(&root, &["add", "main-only.rs"]);
        git_ok(
            &root,
            &[
                "-c",
                "user.name=Goddard Tests",
                "-c",
                "user.email=waku@example.com",
                "commit",
                "-m",
                "main baseline",
            ],
        );

        let session_id = Uuid::new_v4();
        crate::checkpoint::capture_turn_start(&root, session_id, 1).unwrap();
        git_ok(&root, &["switch", "feature"]);
        fs::write(
            root.join("src/lib.rs"),
            "fn baseline() {}\nfn from_turn() {}\n",
        )
        .unwrap();
        crate::checkpoint::capture_turn(&root, session_id, 1).unwrap();

        let data = collect(
            &root,
            ReviewDiffSource::LastTurn {
                session_id,
                turn_id: Uuid::new_v4(),
                turn_count: 1,
            },
        );
        assert_eq!(numstat_summary(&data.numstat), (1, 1, 0));
        assert!(data.numstat.ends_with("\tsrc/lib.rs\n"));
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn reclaim_reports_and_deletes_only_ignored_allowlisted_directories() {
        let root = repository();
        fs::write(
            root.join(".gitignore"),
            "node_modules/\ntarget/\ntemp/\nscratch.txt\n",
        )
        .unwrap();
        git_ok(&root, &["add", ".gitignore"]);
        git_ok(
            &root,
            &[
                "-c",
                "user.name=Goddard Tests",
                "-c",
                "user.email=waku@example.com",
                "commit",
                "-m",
                "ignore rules",
            ],
        );
        // A tracked `build/` stays off the list even though the name is
        // allowlisted — the ignored gate is the safety hinge.
        fs::write(root.join("build.rs"), "fn main() {}\n").unwrap();
        fs::create_dir_all(root.join("build")).unwrap();
        fs::write(root.join("build/tool.rs"), "fn tool() {}\n").unwrap();
        git_ok(&root, &["add", "build.rs", "build/tool.rs"]);
        fs::create_dir_all(root.join("node_modules/dep")).unwrap();
        fs::write(root.join("node_modules/dep/index.js"), "x".repeat(100)).unwrap();
        fs::create_dir_all(root.join("target/debug")).unwrap();
        fs::write(root.join("target/debug/app"), "y".repeat(50)).unwrap();
        fs::create_dir_all(root.join("temp")).unwrap();
        fs::write(root.join("temp/scratch.txt"), "z").unwrap();

        let WorkspaceResult::Reclaimable { entries } = execute(
            WorkspaceOperation::InspectReclaimable { cwd: root.clone() },
            "qa",
        )
        .unwrap() else {
            panic!("unexpected workspace response")
        };
        let mut names = entries
            .iter()
            .map(|entry| {
                entry
                    .path
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect::<Vec<_>>();
        names.sort();
        assert_eq!(names, ["node_modules", "target"]);
        let node = entries
            .iter()
            .find(|entry| entry.path.ends_with("node_modules"))
            .unwrap();
        assert!(node.bytes >= 100);

        // A tracked path smuggled into the request is skipped, not
        // deleted — the daemon re-derives the safe set.
        let mut paths = entries
            .iter()
            .map(|entry| entry.path.clone())
            .collect::<Vec<_>>();
        paths.push(root.join("build"));
        paths.push(root.join("src"));
        let WorkspaceResult::Reclaim {
            reclaimed_bytes,
            failures,
        } = execute(
            WorkspaceOperation::ReclaimPaths {
                cwd: root.clone(),
                paths,
            },
            "qa",
        )
        .unwrap()
        else {
            panic!("unexpected workspace response")
        };
        assert!(failures.is_empty());
        assert!(reclaimed_bytes >= 150);
        assert!(!root.join("node_modules").exists());
        assert!(!root.join("target").exists());
        assert!(root.join("build/tool.rs").exists());
        assert!(root.join("temp/scratch.txt").exists());
        assert!(root.join("src/lib.rs").exists());
        fs::remove_dir_all(root).ok();
    }
}
