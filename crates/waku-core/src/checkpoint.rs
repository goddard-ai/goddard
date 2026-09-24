//! Daemon-owned Git checkpoint capture and restoration.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::ffi::OsStr;
use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Output, Stdio};
use std::sync::{Arc, OnceLock};

use anyhow::{Context as _, anyhow, bail};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::model::{Checkpoint, CheckpointFile, CheckpointStatus, unix_time};

const TURN_START_METADATA_PREFIX: &str = "Waku-Turn-Start: ";
const EMPTY_TREE: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";
/// The side index every snapshot on a worktree shares, kept in the worktree's
/// own git dir so its stat cache stays valid for that checkout. Reuse is what
/// makes `git add` incremental: entries whose stat is unchanged skip hashing,
/// so the per-capture cost tracks changed files instead of worktree size.
const CHECKPOINT_INDEX_NAME: &str = "waku-checkpoint-index";

#[derive(Debug, Deserialize, Serialize)]
struct TurnStartMetadata {
    head: Option<String>,
    branch: Option<String>,
    refs: BTreeMap<String, String>,
}

pub fn checkpoint_ref(session_id: Uuid, turn_count: usize) -> String {
    format!("refs/waku/session-{session_id}-turn-{turn_count}")
}

pub fn turn_start_ref(session_id: Uuid, turn_count: usize) -> String {
    format!("refs/waku/session-{session_id}-turn-start-{turn_count}")
}

pub fn turn_diff_base_ref(session_id: Uuid, turn_count: usize) -> String {
    format!("refs/waku/session-{session_id}-turn-diff-{turn_count}")
}

/// The commit `HEAD` named when the turn started — a cheap marker written
/// before the full start snapshot so a failed or abandoned capture still
/// leaves a diff base. It carries no worktree state, so diffing against it
/// charges files that were already dirty to the turn.
pub fn turn_base_ref(session_id: Uuid, turn_count: usize) -> String {
    format!("refs/waku/session-{session_id}-turn-base-{turn_count}")
}

/// Snapshot of the whole worktree taken when a session is archived, so the
/// worktree can be removed without losing work. Living under the shared
/// `refs/waku/session-<id>-` prefix means session ref deletion — including
/// the archived-session retention purge — collects it with the rest.
pub fn archive_ref(session_id: Uuid) -> String {
    format!("refs/waku/session-{session_id}-archive")
}

/// Capture the exact workspace state accepted for a turn before its provider
/// starts. This is intentionally distinct from the preceding turn's ending
/// checkpoint: a branch switch or terminal edit between turns must not be
/// attributed to either response.
pub fn capture_turn_start(cwd: &Path, session_id: Uuid, turn_count: usize) -> anyhow::Result<()> {
    if !is_git_repository(cwd) {
        return Ok(());
    }

    let head = resolve_ref(cwd, "HEAD");
    let branch = symbolic_head(cwd);
    // The base marker costs one ref update and lands before the expensive
    // snapshot: a client timeout or mid-capture failure then still leaves the
    // ending diff a usable base.
    if let Some(head) = head.as_ref() {
        update_refs(
            cwd,
            format!("update {} {head}\n", turn_base_ref(session_id, turn_count)),
        )?;
    }
    let refs = repository_refs(cwd)?;
    let metadata = TurnStartMetadata {
        head: head.clone(),
        branch,
        refs,
    };
    let message = format!(
        "Goddard turn start snapshot\n\n{TURN_START_METADATA_PREFIX}{}",
        serde_json::to_string(&metadata)?
    );
    let mut parents = Vec::new();
    let mut seen = HashSet::new();
    if let Some(head) = head.as_ref()
        && seen.insert(head.clone())
    {
        parents.push(head.clone());
    }
    for commit in metadata.refs.values() {
        if seen.insert(commit.clone()) {
            parents.push(commit.clone());
        }
    }
    let commit = capture_worktree_commit_from(cwd, head.as_deref(), &message, &parents)?;
    let start_ref = turn_start_ref(session_id, turn_count);
    let baseline_ref = checkpoint_ref(session_id, turn_count.saturating_sub(1));
    let mut commands = format!("update {start_ref} {commit}\n");
    if !has_ref(cwd, &baseline_ref) {
        commands.push_str(&format!("update {baseline_ref} {commit}\n"));
    }
    update_refs(cwd, commands)
}

pub fn capture_turn(cwd: &Path, session_id: Uuid, turn_count: usize) -> anyhow::Result<Checkpoint> {
    let git_ref = checkpoint_ref(session_id, turn_count);
    if !is_git_repository(cwd) {
        return Ok(Checkpoint {
            turn_count,
            git_ref,
            status: CheckpointStatus::Unavailable,
            files: Vec::new(),
            additions: 0,
            deletions: 0,
            created_at: unix_time(),
        });
    }

    let end_branch = symbolic_head(cwd);
    let end_head = resolve_ref(cwd, "HEAD");
    let start_ref = turn_start_ref(session_id, turn_count);
    let has_start_ref = turn_count > 0 && has_ref(cwd, &start_ref);
    let end_commit = if has_start_ref {
        capture_worktree_commit_from_turn_start(cwd, &start_ref)?
    } else {
        capture_worktree_commit_from(cwd, end_head.as_deref(), "Goddard worktree snapshot", &[])?
    };
    git_output(cwd, ["update-ref", &git_ref, &end_commit])?;
    let files = if turn_count == 0 {
        Vec::new()
    } else {
        let legacy_ref = checkpoint_ref(session_id, turn_count - 1);
        let diff_base = if has_start_ref {
            prepare_turn_diff_base(
                cwd,
                session_id,
                turn_count,
                &start_ref,
                end_head.as_deref(),
                end_branch.as_deref(),
            )?
        } else if has_ref(cwd, &legacy_ref) {
            legacy_ref
        } else {
            // The start snapshot never landed — the pre-turn HEAD marker is
            // the closest remaining base.
            turn_base_ref(session_id, turn_count)
        };
        if has_ref(cwd, &diff_base) {
            diff_files(cwd, &diff_base, &git_ref)?
        } else {
            Vec::new()
        }
    };
    let additions = files.iter().map(|file| file.additions).sum();
    let deletions = files.iter().map(|file| file.deletions).sum();
    Ok(Checkpoint {
        turn_count,
        git_ref,
        status: CheckpointStatus::Ready,
        files,
        additions,
        deletions,
        created_at: unix_time(),
    })
}

/// An interrupted turn that never ran a tool can use its starting snapshot.
/// Preserve the existing dirty worktree in that snapshot without scanning it
/// a second time. A moved HEAD or branch needs the normal branch-aware capture.
pub fn capture_untouched_turn(
    cwd: &Path,
    session_id: Uuid,
    turn_count: usize,
) -> anyhow::Result<Option<Checkpoint>> {
    let start_ref = turn_start_ref(session_id, turn_count);
    if !has_ref(cwd, &start_ref) {
        return Ok(None);
    }
    let metadata = turn_start_metadata(cwd, &start_ref)?;
    if metadata.head != resolve_ref(cwd, "HEAD") || metadata.branch != symbolic_head(cwd) {
        return Ok(None);
    }
    let git_ref = checkpoint_ref(session_id, turn_count);
    let start_commit = resolve_ref(cwd, &start_ref)
        .ok_or_else(|| anyhow!("turn starting checkpoint `{start_ref}` is unavailable"))?;
    git_output(cwd, ["update-ref", &git_ref, &start_commit])?;
    Ok(Some(Checkpoint {
        turn_count,
        git_ref,
        status: CheckpointStatus::Ready,
        files: Vec::new(),
        additions: 0,
        deletions: 0,
        created_at: unix_time(),
    }))
}

/// Snapshot the whole worktree — including untracked files — into `git_ref`.
/// The commit's first parent is the checkout's HEAD, so a worktree recreated
/// from the ref can come up on the real commit with the snapshot's
/// difference replayed as uncommitted work rather than as a detached root
/// commit with no history.
pub fn capture_ref(cwd: &Path, git_ref: &str) -> anyhow::Result<()> {
    if !is_git_repository(cwd) {
        bail!("checkpoints require a Git repository");
    }

    let head = resolve_ref(cwd, "HEAD");
    let parents = head.iter().cloned().collect::<Vec<_>>();
    let commit =
        capture_worktree_commit_from(cwd, head.as_deref(), "Goddard worktree snapshot", &parents)?;
    git_output(cwd, ["update-ref", git_ref, &commit])?;
    Ok(())
}

/// Capture the current worktree and untracked files as a dangling commit.
///
/// This shares the checkpoint path's isolated per-worktree index, so it
/// never stages or unstages the user's files. Review uses the returned
/// treeish to compare a stable worktree snapshot while edits continue on
/// disk.
pub fn capture_worktree_commit(cwd: &Path) -> anyhow::Result<String> {
    if !is_git_repository(cwd) {
        bail!("worktree snapshots require a Git repository");
    }

    let head = resolve_ref(cwd, "HEAD");
    capture_worktree_commit_from(cwd, head.as_deref(), "Goddard worktree snapshot", &[])
}

fn capture_worktree_commit_from(
    cwd: &Path,
    head: Option<&str>,
    message: &str,
    parents: &[String],
) -> anyhow::Result<String> {
    capture_worktree_commit_with_base(cwd, head, message, parents, None)
}

fn capture_worktree_commit_from_turn_start(cwd: &Path, start_ref: &str) -> anyhow::Result<String> {
    capture_worktree_commit_with_base(cwd, None, "Goddard worktree snapshot", &[], Some(start_ref))
}

fn capture_worktree_commit_with_base(
    cwd: &Path,
    head: Option<&str>,
    message: &str,
    parents: &[String],
    start_ref: Option<&str>,
) -> anyhow::Result<String> {
    if !is_git_repository(cwd) {
        bail!("worktree snapshots require a Git repository");
    }

    let git_dir = worktree_git_dir(cwd)?;
    let index = git_dir.join(CHECKPOINT_INDEX_NAME);
    // Every snapshot writes through the same side index, so two captures on
    // one worktree must not overlap. Captures on different worktrees keep
    // their own indexes and stay independent.
    let capture_lock = capture_lock(&git_dir);
    let _capture = capture_lock.lock();
    // A capture killed mid-write leaves its index lock behind; the mutex
    // above makes any leftover dead.
    let _ = fs::remove_file(index.with_extension("lock"));

    match capture_with_index(cwd, &index, head, message, parents, start_ref) {
        Err(_) => {
            // A stale or corrupt side index fails plumbing the caller cannot
            // fix — rebuild it once from scratch before giving up.
            let _ = fs::remove_file(&index);
            let _ = fs::remove_file(index.with_extension("lock"));
            capture_with_index(cwd, &index, head, message, parents, start_ref)
        }
        ok => ok,
    }
}

fn capture_with_index(
    cwd: &Path,
    index: &Path,
    head: Option<&str>,
    message: &str,
    parents: &[String],
    start_ref: Option<&str>,
) -> anyhow::Result<String> {
    if let Some(start_ref) = start_ref {
        return capture_with_turn_start_index(cwd, index, start_ref, message, parents);
    }

    // A clean worktree commits HEAD's tree outright — no index writes and no
    // hashing at all. Status runs against the side index, which keeps it off
    // the user's index while still giving it a warm stat cache.
    let changed_paths = worktree_status(cwd, index)?;
    if let Some(head) = head
        && changed_paths.is_empty()
    {
        let tree = git_output(cwd, ["rev-parse", &format!("{head}^{{tree}}")])?
            .trim()
            .to_owned();
        if tree.is_empty() {
            bail!("git rev-parse returned no tree id");
        }
        return commit_tree(cwd, &tree, message, parents);
    }
    if let Some(head) = head {
        git_with_index(cwd, index, ["read-tree", head])?;
    }
    if head.is_some() {
        if let Some(pathspecs) = status_pathspecs(&changed_paths)
            && !pathspecs.is_empty()
        {
            git_with_index_input(
                cwd,
                index,
                ["add", "-A", "--pathspec-from-file=-", "--pathspec-file-nul"],
                &pathspecs,
            )?;
        } else {
            // An unfamiliar or empty status record must never omit files from
            // a checkpoint. Keep the full capture as the safe fallback.
            git_with_index(cwd, index, ["add", "-A", "--", "."])?;
        }
    } else {
        git_with_index(cwd, index, ["add", "-A", "--", "."])?;
    }
    let tree = git_with_index(cwd, index, ["write-tree"])?
        .trim()
        .to_owned();
    if tree.is_empty() {
        bail!("git write-tree returned no object id");
    }
    commit_tree(cwd, &tree, message, parents)
}

fn capture_with_turn_start_index(
    cwd: &Path,
    index: &Path,
    start_ref: &str,
    message: &str,
    parents: &[String],
) -> anyhow::Result<String> {
    git_with_index(cwd, index, ["read-tree", start_ref])?;
    let mut pathspecs = Vec::new();
    for args in [
        &[
            "diff-files",
            "--name-only",
            "-z",
            "--ignore-submodules=none",
            "--",
            ".",
        ][..],
        &[
            "ls-files",
            "--others",
            "--exclude-standard",
            "--directory",
            "-z",
            "--",
            ".",
        ][..],
    ] {
        let paths = git_with_index_output(cwd, index, args)?.stdout;
        for path in paths
            .split(|byte| *byte == 0)
            .filter(|path| !path.is_empty())
        {
            push_literal_pathspec(&mut pathspecs, path);
        }
    }

    if pathspecs.is_empty() {
        return resolve_ref(cwd, start_ref)
            .ok_or_else(|| anyhow!("turn starting checkpoint `{start_ref}` is unavailable"));
    }

    git_with_index_input(
        cwd,
        index,
        ["add", "-A", "--pathspec-from-file=-", "--pathspec-file-nul"],
        &pathspecs,
    )?;
    let tree = git_with_index(cwd, index, ["write-tree"])?
        .trim()
        .to_owned();
    if tree.is_empty() {
        bail!("git write-tree returned no object id");
    }
    commit_tree(cwd, &tree, message, parents)
}

/// Changes relative to the side index, including staged, unstaged, and
/// non-ignored untracked paths. `-z` preserves arbitrary path bytes for the
/// pathspec file. `--untracked-files=all` keeps `status.showUntrackedFiles`
/// from hiding files the snapshot must see.
fn worktree_status(cwd: &Path, index: &Path) -> anyhow::Result<Vec<u8>> {
    Ok(git_with_index_output(
        cwd,
        index,
        [
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=all",
            "--",
            ".",
        ],
    )?
    .stdout)
}

/// Convert porcelain-v1 NUL records into literal pathspecs suitable for
/// `git add --pathspec-from-file`. Rename/copy records carry a second path;
/// including both sides also removes the old name from the new tree.
fn status_pathspecs(status: &[u8]) -> Option<Vec<u8>> {
    let mut records = status.split(|byte| *byte == 0);
    let mut pathspecs = Vec::new();
    while let Some(record) = records.next() {
        if record.is_empty() {
            continue;
        }
        if record.len() < 4 || record[2] != b' ' {
            return None;
        }
        push_literal_pathspec(&mut pathspecs, &record[3..]);
        if matches!(record[0], b'R' | b'C') || matches!(record[1], b'R' | b'C') {
            let original_path = records.next()?;
            if original_path.is_empty() {
                return None;
            }
            push_literal_pathspec(&mut pathspecs, original_path);
        }
    }
    Some(pathspecs)
}

fn push_literal_pathspec(pathspecs: &mut Vec<u8>, path: &[u8]) {
    pathspecs.extend_from_slice(b":(literal)");
    pathspecs.extend_from_slice(path);
    pathspecs.push(0);
}

/// The worktree's own admin dir — `.git` on a normal checkout, the
/// per-worktree dir under the common `.git` on a linked one — where the side
/// index lives.
fn worktree_git_dir(cwd: &Path) -> anyhow::Result<PathBuf> {
    let dir = git_output(cwd, ["rev-parse", "--git-dir"])?
        .trim()
        .to_owned();
    if dir.is_empty() {
        bail!("git did not return its directory");
    }
    let dir = PathBuf::from(dir);
    Ok(if dir.is_absolute() {
        dir
    } else {
        cwd.join(dir)
    })
}

/// One lock per worktree git dir, so snapshots sharing a side index can never
/// interleave their index writes.
fn capture_lock(git_dir: &Path) -> Arc<Mutex<()>> {
    static LOCKS: OnceLock<Mutex<HashMap<PathBuf, Arc<Mutex<()>>>>> = OnceLock::new();
    // Symlinked spellings of one git dir must share a lock.
    let key = git_dir
        .canonicalize()
        .unwrap_or_else(|_| git_dir.to_path_buf());
    LOCKS
        .get_or_init(Mutex::default)
        .lock()
        .entry(key)
        .or_insert_with(|| Arc::new(Mutex::new(())))
        .clone()
}

fn prepare_turn_diff_base(
    cwd: &Path,
    session_id: Uuid,
    turn_count: usize,
    start_ref: &str,
    end_head: Option<&str>,
    end_branch: Option<&str>,
) -> anyhow::Result<String> {
    let metadata = turn_start_metadata(cwd, start_ref)?;
    let same_line = match (metadata.branch.as_deref(), end_branch) {
        (Some(start), Some(end)) => start == end,
        (None, None) => metadata.head.as_deref() == end_head,
        _ => false,
    };

    let commit = if same_line {
        resolve_ref(cwd, start_ref)
            .ok_or_else(|| anyhow!("turn starting checkpoint `{start_ref}` is unavailable"))?
    } else {
        let target_base = target_branch_start(cwd, start_ref, &metadata, end_head, end_branch)?;
        virtual_branch_start(cwd, start_ref, metadata.head.as_deref(), &target_base)?
    };
    let diff_ref = turn_diff_base_ref(session_id, turn_count);
    git_output(cwd, ["update-ref", &diff_ref, &commit])?;
    Ok(diff_ref)
}

fn turn_start_metadata(cwd: &Path, start_ref: &str) -> anyhow::Result<TurnStartMetadata> {
    let message = git_output(cwd, ["show", "-s", "--format=%B", start_ref])?;
    let encoded = message
        .lines()
        .find_map(|line| line.strip_prefix(TURN_START_METADATA_PREFIX))
        .ok_or_else(|| anyhow!("turn starting checkpoint metadata is unavailable"))?;
    serde_json::from_str(encoded).context("invalid turn starting checkpoint metadata")
}

fn target_branch_start(
    cwd: &Path,
    start_ref: &str,
    metadata: &TurnStartMetadata,
    end_head: Option<&str>,
    end_branch: Option<&str>,
) -> anyhow::Result<String> {
    if let Some(branch) = end_branch
        && let Some(commit) = metadata.refs.get(branch)
    {
        return Ok(commit.clone());
    }
    let Some(end_head) = end_head else {
        return empty_tree_commit(cwd);
    };
    let new_commits = git_output(
        cwd,
        [
            "rev-list",
            "--first-parent",
            "--reverse",
            end_head,
            "--not",
            start_ref,
        ],
    )?;
    let Some(first_new_commit) = new_commits.lines().find(|line| !line.trim().is_empty()) else {
        return Ok(end_head.to_owned());
    };
    resolve_ref(cwd, &format!("{first_new_commit}^1")).map_or_else(|| empty_tree_commit(cwd), Ok)
}

fn virtual_branch_start(
    cwd: &Path,
    start_ref: &str,
    start_head: Option<&str>,
    target_base: &str,
) -> anyhow::Result<String> {
    let Some(start_head) = start_head else {
        return Ok(target_base.to_owned());
    };
    if start_head == target_base {
        return resolve_ref(cwd, start_ref)
            .ok_or_else(|| anyhow!("turn starting checkpoint `{start_ref}` is unavailable"));
    }
    if git_output(cwd, ["diff", "--name-only", start_head, start_ref])?
        .trim()
        .is_empty()
    {
        return Ok(target_base.to_owned());
    }

    // Recreate the state Git would have after carrying the user's pre-turn
    // dirty files onto the target branch. Comparing against the raw target tip
    // would otherwise attribute those already-present edits to the response.
    let output = git_output(
        cwd,
        [
            "merge-tree",
            "--write-tree",
            "--merge-base",
            start_head,
            target_base,
            start_ref,
        ],
    )?;
    let tree = output
        .lines()
        .next()
        .map(str::trim)
        .filter(|tree| !tree.is_empty())
        .ok_or_else(|| anyhow!("git merge-tree returned no tree"))?;
    commit_tree(cwd, tree, "Goddard turn diff base", &[])
}

fn empty_tree_commit(cwd: &Path) -> anyhow::Result<String> {
    commit_tree(cwd, EMPTY_TREE, "Goddard empty turn diff base", &[])
}

fn commit_tree(
    cwd: &Path,
    tree: &str,
    message: &str,
    parents: &[String],
) -> anyhow::Result<String> {
    let mut arguments = vec![
        "commit-tree".to_owned(),
        tree.to_owned(),
        "-m".to_owned(),
        message.to_owned(),
    ];
    for parent in parents {
        arguments.push("-p".to_owned());
        arguments.push(parent.clone());
    }
    let commit = git_with_identity(cwd, &arguments)?.trim().to_owned();
    if commit.is_empty() {
        bail!("git commit-tree returned no object id");
    }
    Ok(commit)
}

pub fn restore_ref(cwd: &Path, git_ref: &str) -> anyhow::Result<()> {
    let commit = resolve_ref(cwd, git_ref)
        .ok_or_else(|| anyhow!("checkpoint `{git_ref}` is unavailable"))?;
    git_output(
        cwd,
        [
            "restore",
            "--source",
            &commit,
            "--worktree",
            "--staged",
            "--",
            ".",
        ],
    )?;
    git_output(cwd, ["clean", "-fd", "--", "."])?;
    if has_head(cwd) {
        git_output(cwd, ["reset", "--quiet", "--", "."])?;
    }
    Ok(())
}

pub fn has_ref(cwd: &Path, git_ref: &str) -> bool {
    resolve_ref(cwd, git_ref).is_some()
}

/// Every turn count that has a checkpoint ref for `session_id`, resolved with
/// a single `git for-each-ref` instead of one `git rev-parse` per turn.
pub fn session_turn_refs(cwd: &Path, session_id: Uuid) -> HashSet<usize> {
    session_checkpoint_ref_commits(cwd, session_id)
        .turns
        .into_keys()
        .collect()
}

pub fn delete_ref(cwd: &Path, git_ref: &str) -> anyhow::Result<()> {
    let output = crate::command_env::search_path_command("git")
        .args(["update-ref", "-d", git_ref])
        .current_dir(cwd)
        .output()
        .with_context(|| format!("failed to delete checkpoint `{git_ref}`"))?;
    if output.status.success() {
        Ok(())
    } else {
        bail!("{}", command_error(&output))
    }
}

pub fn delete_turn_refs_after(
    cwd: &Path,
    session_id: Uuid,
    retained_turn_count: usize,
    previous_turn_count: usize,
) -> anyhow::Result<()> {
    let mut commands = String::new();
    for turn_count in retained_turn_count + 1..=previous_turn_count {
        commands.push_str(&format!(
            "delete {}\ndelete {}\ndelete {}\ndelete {}\n",
            checkpoint_ref(session_id, turn_count),
            turn_start_ref(session_id, turn_count),
            turn_diff_base_ref(session_id, turn_count),
            turn_base_ref(session_id, turn_count)
        ));
    }
    update_refs(cwd, commands)
}

pub fn delete_session_refs(
    cwd: &Path,
    session_id: Uuid,
    last_turn_count: usize,
) -> anyhow::Result<()> {
    let mut commands = String::new();
    for turn_count in 0..=last_turn_count {
        commands.push_str(&format!(
            "delete {}\n",
            checkpoint_ref(session_id, turn_count)
        ));
        if turn_count > 0 {
            commands.push_str(&format!(
                "delete {}\ndelete {}\ndelete {}\n",
                turn_start_ref(session_id, turn_count),
                turn_diff_base_ref(session_id, turn_count),
                turn_base_ref(session_id, turn_count)
            ));
        }
    }
    update_refs(cwd, commands)
}

/// Delete every checkpoint ref owned by a session without requiring a client
/// to know how many turns are stored. This is the remote-safe deletion path:
/// the daemon enumerates its own repository refs as the authority.
pub fn delete_all_session_refs(cwd: &Path, session_id: Uuid) -> anyhow::Result<()> {
    let refs = session_checkpoint_ref_commits(cwd, session_id);
    let mut commands = String::new();
    for turn_count in refs.turns.keys() {
        commands.push_str(&format!(
            "delete {}\n",
            checkpoint_ref(session_id, *turn_count)
        ));
    }
    for turn_count in refs.starts.keys() {
        commands.push_str(&format!(
            "delete {}\n",
            turn_start_ref(session_id, *turn_count)
        ));
    }
    for turn_count in refs.diff_bases.keys() {
        commands.push_str(&format!(
            "delete {}\n",
            turn_diff_base_ref(session_id, *turn_count)
        ));
    }
    for turn_count in refs.bases.keys() {
        commands.push_str(&format!(
            "delete {}\n",
            turn_base_ref(session_id, *turn_count)
        ));
    }
    update_refs(cwd, commands)
}

pub fn copy_session_refs(
    cwd: &Path,
    source_session_id: Uuid,
    target_session_id: Uuid,
    through_turn_count: usize,
) -> anyhow::Result<()> {
    if !is_git_repository(cwd) {
        return Ok(());
    }

    let source = session_checkpoint_ref_commits(cwd, source_session_id);
    let mut commands = String::new();
    for turn_count in 0..=through_turn_count {
        if let Some(commit) = source.turns.get(&turn_count) {
            commands.push_str(&format!(
                "update {} {commit}\n",
                checkpoint_ref(target_session_id, turn_count)
            ));
        }
        if turn_count == 0 {
            continue;
        }
        if let Some(commit) = source.starts.get(&turn_count) {
            commands.push_str(&format!(
                "update {} {commit}\n",
                turn_start_ref(target_session_id, turn_count)
            ));
        }
        if let Some(commit) = source.diff_bases.get(&turn_count) {
            commands.push_str(&format!(
                "update {} {commit}\n",
                turn_diff_base_ref(target_session_id, turn_count)
            ));
        }
        if let Some(commit) = source.bases.get(&turn_count) {
            commands.push_str(&format!(
                "update {} {commit}\n",
                turn_base_ref(target_session_id, turn_count)
            ));
        }
    }
    update_refs(cwd, commands)
}

#[derive(Default)]
struct SessionCheckpointRefs {
    turns: HashMap<usize, String>,
    starts: HashMap<usize, String>,
    diff_bases: HashMap<usize, String>,
    bases: HashMap<usize, String>,
}

/// Every checkpoint ref for `session_id`, resolved in one `git for-each-ref`.
fn session_checkpoint_ref_commits(cwd: &Path, session_id: Uuid) -> SessionCheckpointRefs {
    let prefix = format!("refs/waku/session-{session_id}-");
    git_output(
        cwd,
        [
            "for-each-ref",
            "--format=%(refname) %(objectname)",
            &format!("{prefix}*"),
        ],
    )
    .map(|output| {
        let mut refs = SessionCheckpointRefs::default();
        for line in output.lines() {
            let Some((refname, commit)) = line.trim().split_once(' ') else {
                continue;
            };
            let Some(suffix) = refname.strip_prefix(prefix.as_str()) else {
                continue;
            };
            let target = if let Some(turn_count) = suffix.strip_prefix("turn-base-") {
                turn_count
                    .parse()
                    .ok()
                    .map(|turn_count| (&mut refs.bases, turn_count))
            } else if let Some(turn_count) = suffix.strip_prefix("turn-start-") {
                turn_count
                    .parse()
                    .ok()
                    .map(|turn_count| (&mut refs.starts, turn_count))
            } else if let Some(turn_count) = suffix.strip_prefix("turn-diff-") {
                turn_count
                    .parse()
                    .ok()
                    .map(|turn_count| (&mut refs.diff_bases, turn_count))
            } else if let Some(turn_count) = suffix.strip_prefix("turn-") {
                turn_count
                    .parse()
                    .ok()
                    .map(|turn_count| (&mut refs.turns, turn_count))
            } else {
                None
            };
            if let Some((target, turn_count)) = target {
                target.insert(turn_count, commit.to_owned());
            }
        }
        refs
    })
    .unwrap_or_default()
}

/// Applies a batch of ref updates through one `git update-ref --stdin`.
///
/// Deleting a fifty-turn session's checkpoints was fifty `git` invocations run
/// serially on the thread that had just been asked to remove the session. The
/// batch form is one process, and it is atomic: either the whole set applies or
/// none of it does. Deleting a ref that is already gone is not an error.
///
/// Checkpoint ref names are generated, never user text, so they cannot contain
/// the space or newline this line-oriented format delimits on.
fn update_refs(cwd: &Path, commands: String) -> anyhow::Result<()> {
    if commands.is_empty() {
        return Ok(());
    }
    let mut child = crate::command_env::search_path_command("git")
        .args(["update-ref", "--stdin"])
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("failed to execute git")?;
    child
        .stdin
        .take()
        .ok_or_else(|| anyhow!("git update-ref stdin is unavailable"))?
        .write_all(commands.as_bytes())
        .context("failed to send ref updates to git")?;
    let output = child.wait_with_output().context("failed to execute git")?;
    if output.status.success() {
        Ok(())
    } else {
        bail!("{}", command_error(&output))
    }
}

fn diff_files(cwd: &Path, from_ref: &str, to_ref: &str) -> anyhow::Result<Vec<CheckpointFile>> {
    let output = git_output(cwd, ["diff", "--numstat", from_ref, to_ref, "--", "."])?;
    let mut files = Vec::new();
    for line in output.lines().filter(|line| !line.trim().is_empty()) {
        let mut columns = line.splitn(3, '\t');
        let additions = columns.next().unwrap_or("0").parse().unwrap_or(0);
        let deletions = columns.next().unwrap_or("0").parse().unwrap_or(0);
        let Some(path) = columns.next() else {
            continue;
        };
        files.push(CheckpointFile {
            path: path.to_owned(),
            additions,
            deletions,
        });
    }
    Ok(files)
}

fn is_git_repository(cwd: &Path) -> bool {
    crate::command_env::search_path_command("git")
        .args(["rev-parse", "--is-inside-work-tree"])
        .current_dir(cwd)
        .output()
        .is_ok_and(|output| output.status.success())
}

fn symbolic_head(cwd: &Path) -> Option<String> {
    let output = crate::command_env::search_path_command("git")
        .args(["symbolic-ref", "--quiet", "HEAD"])
        .current_dir(cwd)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .filter(|branch| !branch.is_empty())
}

fn repository_refs(cwd: &Path) -> anyhow::Result<BTreeMap<String, String>> {
    let output = git_output(
        cwd,
        [
            "for-each-ref",
            "--format=%(refname)%09%(objecttype)%09%(objectname)%09%(*objecttype)%09%(*objectname)",
            "refs/heads",
            "refs/remotes",
            "refs/tags",
        ],
    )?;
    Ok(output
        .lines()
        .filter_map(|line| {
            let mut fields = line.trim().split('\t');
            let refname = fields.next()?;
            let object_type = fields.next()?;
            let object = fields.next()?;
            let peeled_type = fields.next().unwrap_or_default();
            let peeled = fields.next().unwrap_or_default();
            let commit = if object_type == "commit" {
                object
            } else if peeled_type == "commit" {
                peeled
            } else {
                return None;
            };
            (!refname.is_empty() && !commit.is_empty())
                .then(|| (refname.to_owned(), commit.to_owned()))
        })
        .collect())
}

fn has_head(cwd: &Path) -> bool {
    crate::command_env::search_path_command("git")
        .args(["rev-parse", "--verify", "HEAD"])
        .current_dir(cwd)
        .output()
        .is_ok_and(|output| output.status.success())
}

fn resolve_ref(cwd: &Path, git_ref: &str) -> Option<String> {
    let output = crate::command_env::search_path_command("git")
        .args(["rev-parse", "--verify", &format!("{git_ref}^{{commit}}")])
        .current_dir(cwd)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn git_output<I, S>(cwd: &Path, args: I) -> anyhow::Result<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
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

fn git_with_index<I, S>(cwd: &Path, index: &Path, args: I) -> anyhow::Result<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let output = git_with_index_output(cwd, index, args)?;
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn git_with_index_output<I, S>(cwd: &Path, index: &Path, args: I) -> anyhow::Result<Output>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let output = crate::command_env::search_path_command("git")
        .args(args)
        .current_dir(cwd)
        .env("GIT_INDEX_FILE", index)
        .output()
        .context("failed to execute git")?;
    if output.status.success() {
        Ok(output)
    } else {
        bail!("{}", command_error(&output))
    }
}

fn git_with_index_input<I, S>(
    cwd: &Path,
    index: &Path,
    args: I,
    input: &[u8],
) -> anyhow::Result<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut child = crate::command_env::search_path_command("git")
        .args(args)
        .current_dir(cwd)
        .env("GIT_INDEX_FILE", index)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("failed to execute git")?;
    child
        .stdin
        .take()
        .ok_or_else(|| anyhow!("git pathspec input was unavailable"))?
        .write_all(input)
        .context("failed to send git pathspecs")?;
    let output = child.wait_with_output().context("failed to wait for git")?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        bail!("{}", command_error(&output))
    }
}

fn git_with_identity<I, S>(cwd: &Path, args: I) -> anyhow::Result<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    git_with_environment(cwd, None, args, true)
}

fn git_with_environment<I, S>(
    cwd: &Path,
    index: Option<&Path>,
    args: I,
    identity: bool,
) -> anyhow::Result<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut command = crate::command_env::search_path_command("git");
    command.args(args).current_dir(cwd);
    if let Some(index) = index {
        command.env("GIT_INDEX_FILE", index);
    }
    if identity {
        command
            .env("GIT_AUTHOR_NAME", "Goddard")
            .env("GIT_AUTHOR_EMAIL", "waku@localhost")
            .env("GIT_COMMITTER_NAME", "Goddard")
            .env("GIT_COMMITTER_EMAIL", "waku@localhost");
    }
    let output = command.output().context("failed to execute git")?;
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
    use super::*;

    fn git_ok(cwd: &Path, args: &[&str]) {
        let status = crate::command_env::search_path_command("git")
            .args(args)
            .current_dir(cwd)
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?} failed");
    }

    fn git_text(cwd: &Path, args: &[&str]) -> String {
        let output = crate::command_env::search_path_command("git")
            .args(args)
            .current_dir(cwd)
            .output()
            .unwrap();
        assert!(output.status.success(), "git {args:?} failed");
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    }

    fn diverged_repository() -> PathBuf {
        let directory = std::env::temp_dir().join(format!("waku-checkpoints-{}", Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap();
        git_ok(&directory, &["init", "--quiet", "--initial-branch=main"]);
        git_ok(&directory, &["config", "user.name", "Goddard Test"]);
        git_ok(&directory, &["config", "user.email", "waku@example.com"]);
        fs::write(directory.join("shared.txt"), "shared\n").unwrap();
        git_ok(&directory, &["add", "shared.txt"]);
        git_ok(&directory, &["commit", "--quiet", "-m", "baseline"]);

        git_ok(&directory, &["switch", "--quiet", "-c", "feature"]);
        fs::write(directory.join("feature-only.txt"), "feature\n").unwrap();
        fs::write(directory.join("target.txt"), "feature baseline\n").unwrap();
        git_ok(&directory, &["add", "feature-only.txt", "target.txt"]);
        git_ok(&directory, &["commit", "--quiet", "-m", "feature"]);

        git_ok(&directory, &["switch", "--quiet", "main"]);
        fs::write(directory.join("main-only.txt"), "main\n").unwrap();
        git_ok(&directory, &["add", "main-only.txt"]);
        git_ok(&directory, &["commit", "--quiet", "-m", "main"]);
        directory
    }

    #[test]
    fn session_turn_refs_lists_the_sessions_checkpoints_in_one_call() {
        let directory = std::env::temp_dir().join(format!("waku-checkpoints-{}", Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap();
        git_ok(&directory, &["init", "--quiet"]);
        fs::write(directory.join("tracked.txt"), "baseline\n").unwrap();
        git_ok(&directory, &["add", "tracked.txt"]);
        git_ok(
            &directory,
            &[
                "-c",
                "user.name=Goddard Test",
                "-c",
                "user.email=waku@example.com",
                "commit",
                "--quiet",
                "-m",
                "baseline",
            ],
        );

        let session = Uuid::new_v4();
        let other = Uuid::new_v4();
        capture_turn(&directory, session, 0).unwrap();
        capture_turn(&directory, session, 2).unwrap();
        capture_turn(&directory, other, 5).unwrap();

        assert_eq!(
            session_turn_refs(&directory, session),
            HashSet::from([0, 2])
        );
        assert_eq!(session_turn_refs(&directory, other), HashSet::from([5]));
        assert!(session_turn_refs(&directory, Uuid::new_v4()).is_empty());
        fs::remove_dir_all(&directory).ok();
    }

    /// Deletes and copies go through one batched `git update-ref --stdin`
    /// rather than a process per turn, so the semantics the loop used to give
    /// for free — gaps are skipped, an already-missing ref is not an error —
    /// are worth pinning down.
    #[test]
    fn refs_are_deleted_and_copied_in_batches() {
        let directory = std::env::temp_dir().join(format!("waku-checkpoints-{}", Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap();
        git_ok(&directory, &["init", "--quiet"]);
        fs::write(directory.join("tracked.txt"), "baseline\n").unwrap();
        git_ok(&directory, &["add", "tracked.txt"]);
        git_ok(
            &directory,
            &[
                "-c",
                "user.name=Goddard Test",
                "-c",
                "user.email=waku@example.com",
                "commit",
                "--quiet",
                "-m",
                "baseline",
            ],
        );

        let session = Uuid::new_v4();
        capture_turn(&directory, session, 0).unwrap();
        capture_turn_start(&directory, session, 1).unwrap();
        capture_turn(&directory, session, 1).unwrap();
        capture_turn(&directory, session, 3).unwrap();
        capture_ref(&directory, &turn_start_ref(session, 3)).unwrap();
        capture_ref(&directory, &turn_diff_base_ref(session, 3)).unwrap();

        // Turn 2 was never captured; the batch must tolerate the gap.
        let fork = Uuid::new_v4();
        copy_session_refs(&directory, session, fork, 3).unwrap();
        assert_eq!(
            session_turn_refs(&directory, fork),
            HashSet::from([0, 1, 3]),
            "a copy carries every ref the source actually has"
        );
        assert_eq!(
            resolve_ref(&directory, &checkpoint_ref(fork, 1)),
            resolve_ref(&directory, &checkpoint_ref(session, 1)),
            "and points at the same commit"
        );
        assert_eq!(
            resolve_ref(&directory, &turn_start_ref(fork, 1)),
            resolve_ref(&directory, &turn_start_ref(session, 1)),
            "the turn's distinct starting snapshot is copied too"
        );
        assert_eq!(
            resolve_ref(&directory, &turn_diff_base_ref(fork, 1)),
            resolve_ref(&directory, &turn_diff_base_ref(session, 1)),
            "the branch-aware review base is copied too"
        );
        assert_eq!(
            resolve_ref(&directory, &turn_base_ref(fork, 1)),
            resolve_ref(&directory, &turn_base_ref(session, 1)),
            "and the cheap head marker follows it"
        );

        delete_turn_refs_after(&directory, session, 1, 3).unwrap();
        assert_eq!(
            session_turn_refs(&directory, session),
            HashSet::from([0, 1]),
            "everything after the retained turn goes, missing ones included"
        );
        assert!(!has_ref(&directory, &turn_start_ref(session, 3)));
        assert!(!has_ref(&directory, &turn_diff_base_ref(session, 3)));
        assert!(!has_ref(&directory, &turn_base_ref(session, 3)));

        delete_session_refs(&directory, fork, 3).unwrap();
        assert!(
            session_turn_refs(&directory, fork).is_empty(),
            "and a session's whole set goes in one call"
        );
        assert!(!has_ref(&directory, &turn_start_ref(fork, 1)));
        assert!(!has_ref(&directory, &turn_diff_base_ref(fork, 1)));
        assert!(!has_ref(&directory, &turn_base_ref(fork, 1)));

        // Nothing left to remove is a no-op, not a failure.
        delete_session_refs(&directory, fork, 3).unwrap();
        fs::remove_dir_all(&directory).ok();
    }

    #[test]
    fn capture_diffs_against_the_head_marker_when_the_start_snapshot_is_missing() {
        let directory = std::env::temp_dir().join(format!("waku-checkpoints-{}", Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap();
        git_ok(&directory, &["init", "--quiet"]);
        fs::write(directory.join("tracked.txt"), "baseline\n").unwrap();
        git_ok(&directory, &["add", "tracked.txt"]);
        git_ok(
            &directory,
            &[
                "-c",
                "user.name=Goddard Test",
                "-c",
                "user.email=waku@example.com",
                "commit",
                "--quiet",
                "-m",
                "baseline",
            ],
        );

        let session = Uuid::new_v4();
        capture_turn_start(&directory, session, 1).unwrap();

        // A start capture that dies late — request timeout, daemon crash —
        // leaves the cheap head marker without the full start snapshot or
        // the usual turn-0 baseline. Both go away here.
        delete_ref(&directory, &turn_start_ref(session, 1)).unwrap();
        delete_ref(&directory, &checkpoint_ref(session, 0)).unwrap();

        fs::write(directory.join("tracked.txt"), "changed\n").unwrap();
        fs::write(directory.join("new.txt"), "added\n").unwrap();

        let checkpoint = capture_turn(&directory, session, 1).unwrap();
        assert_eq!(checkpoint.status, CheckpointStatus::Ready);
        assert!(
            checkpoint
                .files
                .iter()
                .any(|file| file.path == "tracked.txt"),
            "edits still diff against the pre-turn head"
        );
        assert!(
            checkpoint.files.iter().any(|file| file.path == "new.txt"),
            "and so do files the turn created"
        );
        fs::remove_dir_all(&directory).ok();
    }

    #[test]
    fn captures_diffs_and_restores_tracked_and_untracked_files() {
        let directory = std::env::temp_dir().join(format!("waku-checkpoints-{}", Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap();
        git_ok(&directory, &["init", "--quiet"]);
        git_ok(&directory, &["config", "core.autocrlf", "false"]);
        fs::write(directory.join("tracked.txt"), "baseline\n").unwrap();
        git_ok(&directory, &["add", "tracked.txt"]);
        git_ok(
            &directory,
            &[
                "-c",
                "user.name=Goddard Test",
                "-c",
                "user.email=waku@example.com",
                "commit",
                "--quiet",
                "-m",
                "baseline",
            ],
        );

        let session_id = Uuid::new_v4();
        let baseline = capture_turn(&directory, session_id, 0).unwrap();
        assert_eq!(baseline.status, CheckpointStatus::Ready);

        fs::write(directory.join("tracked.txt"), "changed\n").unwrap();
        fs::write(directory.join("new.txt"), "new\n").unwrap();
        fs::write(directory.join("already-staged.txt"), "staged\n").unwrap();
        git_ok(&directory, &["add", "already-staged.txt"]);
        let turn = capture_turn(&directory, session_id, 1).unwrap();
        assert_eq!(turn.files.len(), 3);
        assert!(turn.totals_are_current());
        assert_eq!(
            turn.additions,
            turn.files.iter().map(|file| file.additions).sum::<u64>()
        );
        assert_eq!(
            turn.deletions,
            turn.files.iter().map(|file| file.deletions).sum::<u64>()
        );
        assert_eq!(
            git_text(&directory, &["diff", "--cached", "--name-only"]),
            "already-staged.txt"
        );

        let fork_session_id = Uuid::new_v4();
        copy_session_refs(&directory, session_id, fork_session_id, 1).unwrap();
        assert_eq!(
            resolve_ref(&directory, &checkpoint_ref(fork_session_id, 0)),
            resolve_ref(&directory, &checkpoint_ref(session_id, 0))
        );
        assert_eq!(
            resolve_ref(&directory, &checkpoint_ref(fork_session_id, 1)),
            resolve_ref(&directory, &checkpoint_ref(session_id, 1))
        );

        fs::write(directory.join("tracked.txt"), "later\n").unwrap();
        fs::remove_file(directory.join("new.txt")).unwrap();
        fs::write(directory.join("discard.txt"), "discard\n").unwrap();
        restore_ref(&directory, &turn.git_ref).unwrap();

        assert_eq!(
            fs::read_to_string(directory.join("tracked.txt")).unwrap(),
            "changed\n"
        );
        assert_eq!(
            fs::read_to_string(directory.join("new.txt")).unwrap(),
            "new\n"
        );
        assert!(!directory.join("discard.txt").exists());

        fs::remove_dir_all(directory).ok();
    }

    #[test]
    fn switching_to_an_existing_branch_does_not_report_its_history_as_turn_changes() {
        let directory = diverged_repository();
        let session_id = Uuid::new_v4();
        capture_turn_start(&directory, session_id, 1).unwrap();

        git_ok(&directory, &["switch", "--quiet", "feature"]);
        let turn = capture_turn(&directory, session_id, 1).unwrap();

        assert!(
            turn.files.is_empty(),
            "a branch switch alone is not a file edit: {:?}",
            turn.files
        );
        fs::remove_dir_all(directory).ok();
    }

    #[test]
    fn switching_between_preexisting_detached_commits_does_not_report_their_history() {
        let directory = diverged_repository();
        let older = git_text(&directory, &["rev-parse", "main^1"]);
        let newer = git_text(&directory, &["rev-parse", "main"]);
        git_ok(&directory, &["switch", "--quiet", "--detach", &older]);
        let session_id = Uuid::new_v4();
        capture_turn_start(&directory, session_id, 1).unwrap();

        git_ok(&directory, &["switch", "--quiet", "--detach", &newer]);
        let turn = capture_turn(&directory, session_id, 1).unwrap();

        assert!(
            turn.files.is_empty(),
            "a detached checkout alone is not a file edit: {:?}",
            turn.files
        );
        fs::remove_dir_all(directory).ok();
    }

    #[test]
    fn branch_switch_reports_only_changes_made_on_the_target_branch() {
        let directory = diverged_repository();
        let session_id = Uuid::new_v4();
        capture_turn_start(&directory, session_id, 1).unwrap();

        git_ok(&directory, &["switch", "--quiet", "feature"]);
        fs::write(directory.join("target.txt"), "changed during turn\n").unwrap();
        git_ok(&directory, &["add", "target.txt"]);
        git_ok(&directory, &["commit", "--quiet", "-m", "turn change"]);
        fs::write(directory.join("untracked.txt"), "new during turn\n").unwrap();
        let turn = capture_turn(&directory, session_id, 1).unwrap();

        let paths = turn
            .files
            .iter()
            .map(|file| file.path.as_str())
            .collect::<HashSet<_>>();
        assert_eq!(paths, HashSet::from(["target.txt", "untracked.txt"]));
        assert!(!paths.contains("feature-only.txt"));
        assert!(!paths.contains("main-only.txt"));
        fs::remove_dir_all(directory).ok();
    }

    #[test]
    fn a_branch_created_during_the_turn_uses_its_creation_point_as_the_diff_base() {
        let directory = diverged_repository();
        let session_id = Uuid::new_v4();
        capture_turn_start(&directory, session_id, 1).unwrap();

        git_ok(&directory, &["switch", "--quiet", "-c", "new-branch"]);
        fs::write(directory.join("shared.txt"), "changed on new branch\n").unwrap();
        git_ok(&directory, &["add", "shared.txt"]);
        git_ok(
            &directory,
            &["commit", "--quiet", "-m", "new branch change"],
        );
        let turn = capture_turn(&directory, session_id, 1).unwrap();

        assert_eq!(turn.files.len(), 1);
        assert_eq!(turn.files[0].path, "shared.txt");
        fs::remove_dir_all(directory).ok();
    }

    #[test]
    fn dirty_files_carried_across_a_branch_switch_remain_part_of_the_starting_state() {
        let directory = diverged_repository();
        fs::write(directory.join("shared.txt"), "already dirty\n").unwrap();
        let session_id = Uuid::new_v4();
        capture_turn_start(&directory, session_id, 1).unwrap();

        git_ok(&directory, &["switch", "--quiet", "feature"]);
        let turn = capture_turn(&directory, session_id, 1).unwrap();

        assert!(
            turn.files.is_empty(),
            "pre-turn dirty state carried by Git is not a response edit: {:?}",
            turn.files
        );
        fs::remove_dir_all(directory).ok();
    }

    #[test]
    fn each_turn_uses_its_own_start_snapshot() {
        let directory = diverged_repository();
        let session_id = Uuid::new_v4();
        capture_turn_start(&directory, session_id, 1).unwrap();
        fs::write(directory.join("first-turn.txt"), "first\n").unwrap();
        let first = capture_turn(&directory, session_id, 1).unwrap();
        assert_eq!(first.files.len(), 1);

        fs::write(directory.join("between-turns.txt"), "external\n").unwrap();
        capture_turn_start(&directory, session_id, 2).unwrap();
        fs::write(directory.join("second-turn.txt"), "second\n").unwrap();
        let second = capture_turn(&directory, session_id, 2).unwrap();

        assert_eq!(second.files.len(), 1);
        assert_eq!(second.files[0].path, "second-turn.txt");
        fs::remove_dir_all(directory).ok();
    }

    #[test]
    fn untouched_later_turn_reuses_its_starting_snapshot() {
        let directory = diverged_repository();
        let session_id = Uuid::new_v4();
        capture_turn_start(&directory, session_id, 1).unwrap();
        fs::write(directory.join("first-turn.txt"), "first\n").unwrap();
        capture_turn(&directory, session_id, 1).unwrap();

        capture_turn_start(&directory, session_id, 2).unwrap();
        let checkpoint = capture_untouched_turn(&directory, session_id, 2)
            .unwrap()
            .unwrap();
        assert!(checkpoint.files.is_empty());
        assert_eq!(
            resolve_ref(&directory, &checkpoint.git_ref),
            resolve_ref(&directory, &turn_start_ref(session_id, 2))
        );
        assert_eq!(
            git_output(
                &directory,
                ["show", &format!("{}:first-turn.txt", checkpoint.git_ref)]
            )
            .unwrap(),
            "first\n"
        );

        capture_turn_start(&directory, session_id, 3).unwrap();
        git_ok(&directory, &["switch", "--quiet", "feature"]);
        assert!(
            capture_untouched_turn(&directory, session_id, 3)
                .unwrap()
                .is_none()
        );
        fs::remove_dir_all(directory).ok();
    }

    /// A clean worktree commits HEAD's tree outright — the snapshot skips the
    /// index write and the hash entirely.
    #[test]
    fn a_clean_worktree_snapshots_the_head_tree() {
        let directory = std::env::temp_dir().join(format!("waku-checkpoints-{}", Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap();
        git_ok(&directory, &["init", "--quiet"]);
        fs::write(directory.join("tracked.txt"), "baseline\n").unwrap();
        git_ok(&directory, &["add", "tracked.txt"]);
        git_ok(
            &directory,
            &[
                "-c",
                "user.name=Goddard Test",
                "-c",
                "user.email=waku@example.com",
                "commit",
                "--quiet",
                "-m",
                "baseline",
            ],
        );

        let session = Uuid::new_v4();
        let checkpoint = capture_turn(&directory, session, 0).unwrap();
        assert_eq!(checkpoint.status, CheckpointStatus::Ready);
        assert_eq!(
            git_text(
                &directory,
                &["rev-parse", &format!("{}^{{tree}}", checkpoint.git_ref)]
            ),
            git_text(&directory, &["rev-parse", "HEAD^{tree}"]),
            "a clean worktree's snapshot is HEAD's tree"
        );
        fs::remove_dir_all(directory).ok();
    }

    /// A stale or corrupt side index must not fail the capture — it is
    /// rebuilt once from scratch before the error reaches the caller.
    #[test]
    fn a_corrupt_side_index_rebuilds_itself() {
        let directory = std::env::temp_dir().join(format!("waku-checkpoints-{}", Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap();
        git_ok(&directory, &["init", "--quiet"]);
        fs::write(directory.join("tracked.txt"), "baseline\n").unwrap();
        git_ok(&directory, &["add", "tracked.txt"]);
        git_ok(
            &directory,
            &[
                "-c",
                "user.name=Goddard Test",
                "-c",
                "user.email=waku@example.com",
                "commit",
                "--quiet",
                "-m",
                "baseline",
            ],
        );

        let session = Uuid::new_v4();
        fs::write(directory.join("tracked.txt"), "first change\n").unwrap();
        capture_turn(&directory, session, 0).unwrap();
        let index = worktree_git_dir(&directory)
            .unwrap()
            .join(CHECKPOINT_INDEX_NAME);
        assert!(index.exists(), "a dirty capture leaves the side index");
        fs::write(&index, b"not an index").unwrap();

        fs::write(directory.join("tracked.txt"), "changed\n").unwrap();
        let turn = capture_turn(&directory, session, 1).unwrap();
        assert_eq!(turn.status, CheckpointStatus::Ready);
        assert_eq!(turn.files.len(), 1);
        assert_eq!(turn.files[0].path, "tracked.txt");
        fs::remove_dir_all(directory).ok();
    }
}
