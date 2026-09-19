//! The Git panel's working-tree reads and mutations.
//!
//! Everything here runs on the daemon host inside a workspace request; the
//! panel's render path only ever sees the returned snapshots.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, bail};

use waku_protocol::git::{
    CommitEntry, GitFileChange, GitPanelSnapshot, LandOutcome, LandTarget, PullOutcome,
    PullStrategy, SyncInProgress, UpstreamStatus,
};

use crate::git_branch::remote_url;
use crate::git_commit::{
    command_error, ensure_repository, git_capture, git_optional_stdout, git_stdout, git_success,
    push_target, remote_for_branch, upstream,
};

/// The panel's one-shot state read. `Ok(None)` outside a work tree. `base`
/// is the session's recorded base branch, forwarded to [`land_base`] for the
/// land target.
pub fn inspect(cwd: &Path, base: Option<&str>) -> anyhow::Result<Option<GitPanelSnapshot>> {
    if !git_optional_stdout(cwd, &["rev-parse", "--is-inside-work-tree"])?
        .is_some_and(|answer| answer == "true")
    {
        return Ok(None);
    }
    let branch = git_optional_stdout(cwd, &["branch", "--show-current"])?
        .filter(|branch| !branch.is_empty())
        .or_else(|| {
            git_optional_stdout(cwd, &["rev-parse", "--short", "HEAD"])
                .ok()
                .flatten()
        })
        .unwrap_or_else(|| "HEAD".to_owned());
    let upstream_status = upstream(cwd)?.map(|name| {
        let (ahead, behind) = ahead_behind(cwd, &name).unwrap_or((0, 0));
        UpstreamStatus {
            name,
            ahead,
            behind,
        }
    });
    // Mirrors `git_commit::inspect`: commits the push target lacks, or a
    // remote to publish to when the branch has no upstream yet.
    let can_push = push_target(cwd, &branch)?
        .and_then(|target| {
            git_optional_stdout(cwd, &["rev-list", "--count", &format!("{target}..HEAD")])
                .ok()
                .flatten()
        })
        .and_then(|count| count.parse::<u64>().ok())
        .is_some_and(|count| count > 0)
        || (upstream_status.is_none() && remote_for_branch(cwd, &branch)?.is_some());

    let status = git_stdout(
        cwd,
        &["status", "--porcelain=v1", "-z", "--untracked-files=all"],
    )?;
    let staged_numstat = numstat_map(cwd, &["diff", "--cached", "--numstat", "-z", "--"])?;
    let unstaged_numstat = numstat_map(cwd, &["diff", "--numstat", "-z", "--"])?;

    let mut staged = Vec::new();
    let mut unstaged = Vec::new();
    let mut fields = status.split('\0').filter(|field| !field.is_empty());
    while let Some(field) = fields.next() {
        if field.len() < 4 {
            continue;
        }
        let x = field.as_bytes()[0] as char;
        let y = field.as_bytes()[1] as char;
        let path = field[3..].to_owned();
        // Renames and copies carry a second NUL-separated path: the source.
        if x == 'R' || x == 'C' || y == 'R' || y == 'C' {
            fields.next();
        }
        if x != ' ' && x != '?' {
            let (additions, deletions) = staged_numstat.get(&path).copied().unwrap_or((0, 0));
            staged.push(GitFileChange {
                path: path.clone(),
                status: x.to_string(),
                additions,
                deletions,
                untracked: false,
            });
        }
        if x == '?' {
            unstaged.push(GitFileChange {
                path,
                status: "??".to_owned(),
                additions: 0,
                deletions: 0,
                untracked: true,
            });
        } else if y != ' ' {
            let (additions, deletions) = unstaged_numstat.get(&path).copied().unwrap_or((0, 0));
            unstaged.push(GitFileChange {
                path,
                status: y.to_string(),
                additions,
                deletions,
                untracked: false,
            });
        }
    }
    let base_branch = land_base(cwd, base)?;
    // Where the log's base lane starts: the commit HEAD shares with the
    // branch Land targets. `None` when no base resolves — the panel then
    // draws every commit on the worktree lane.
    let merge_base = base_branch.as_deref().and_then(|branch| {
        git_optional_stdout(cwd, &["merge-base", "HEAD", branch])
            .ok()
            .flatten()
            .filter(|sha| !sha.is_empty())
    });
    let land_target = match base_branch {
        Some(branch) => land_target_on_base(cwd, branch)?,
        None => None,
    };
    Ok(Some(GitPanelSnapshot {
        branch,
        origin_url: remote_url(cwd, "origin")?,
        upstream: upstream_status,
        can_push,
        merge_base,
        land_target,
        staged,
        unstaged,
    }))
}

/// `git add` the whole file.
pub fn stage(cwd: &Path, path: &str) -> anyhow::Result<()> {
    ensure_repository(cwd)?;
    git_success(cwd, &["add", "--", path])?;
    Ok(())
}

/// Unstage whatever of the file is staged. `git reset` needs a HEAD, so a
/// repository with no commits yet falls back to `git rm --cached`.
pub fn unstage(cwd: &Path, path: &str) -> anyhow::Result<()> {
    ensure_repository(cwd)?;
    if ref_exists(cwd, "HEAD")? {
        git_success(cwd, &["reset", "-q", "HEAD", "--", path])?;
    } else {
        git_success(cwd, &["rm", "-q", "--cached", "--", path])?;
    }
    Ok(())
}

/// Integrate upstream: `git pull --rebase` or `--no-rebase`. A clean pull
/// reports `Clean`; a conflicted one leaves the integration in progress and
/// reports `Conflict`, leaving the caller to offer resolve/merge/abort.
pub fn pull(cwd: &Path, strategy: PullStrategy) -> anyhow::Result<PullOutcome> {
    ensure_repository(cwd)?;
    let flag = match strategy {
        PullStrategy::Rebase => "--rebase",
        PullStrategy::Merge => "--no-rebase",
    };
    let output = git_capture(cwd, &["pull", flag])?;
    if output.status.success() {
        return Ok(PullOutcome::Clean);
    }
    if let Some(in_progress) = sync_in_progress(cwd)? {
        return Ok(PullOutcome::Conflict {
            in_progress,
            files: conflicted_paths(cwd)?,
        });
    }
    bail!("{}", command_error(&output))
}

/// The working-tree paths still unmerged — what `git status` shows as
/// "both modified" (or added/deleted). `--diff-filter=U` covers every
/// unmerged status pair.
pub(crate) fn conflicted_paths(cwd: &Path) -> anyhow::Result<Vec<String>> {
    let stdout = git_stdout(cwd, &["diff", "--name-only", "--diff-filter=U", "-z"])?;
    Ok(stdout
        .split('\0')
        .filter(|path| !path.is_empty())
        .map(str::to_owned)
        .collect())
}

/// Abort a conflicted sync: `rebase --abort` while a rebase is stopped,
/// `merge --abort` while a merge is. A no-op when neither is in progress.
pub fn abort_sync(cwd: &Path) -> anyhow::Result<()> {
    match sync_in_progress(cwd)? {
        Some(SyncInProgress::Rebase) => {
            git_success(cwd, &["rebase", "--abort"])?;
        }
        Some(SyncInProgress::Merge) => {
            git_success(cwd, &["merge", "--abort"])?;
        }
        None => {}
    }
    Ok(())
}

/// Whether a rebase or merge stopped on conflicts. Rebase state lives in
/// the `rebase-merge`/`rebase-apply` directories inside the worktree's git
/// dir — `REBASE_HEAD` itself lingers after a stopped rebase is continued
/// to completion, so it cannot stand in for them. `MERGE_HEAD` is removed
/// when the merge concludes, so the ref check is reliable there.
pub(crate) fn sync_in_progress(cwd: &Path) -> anyhow::Result<Option<SyncInProgress>> {
    if git_path_exists(cwd, "rebase-merge")? || git_path_exists(cwd, "rebase-apply")? {
        return Ok(Some(SyncInProgress::Rebase));
    }
    if ref_exists(cwd, "MERGE_HEAD")? {
        return Ok(Some(SyncInProgress::Merge));
    }
    Ok(None)
}

/// Whether `name` exists inside the worktree's git dir. `rev-parse
/// --git-path` resolves per-worktree metadata, so a linked worktree's own
/// `rebase-merge` answers correctly.
fn git_path_exists(cwd: &Path, name: &str) -> anyhow::Result<bool> {
    let path = git_stdout(cwd, &["rev-parse", "--git-path", name])?;
    Ok(cwd.join(path).exists())
}

/// The commit list a landed outcome carries; `ahead` stays the true total
/// when a land sends more.
const LANDED_COMMITS_LIMIT: usize = 50;

/// Land the checkout's commits on its base branch: rebase onto it — or merge
/// it in with `PullStrategy::Merge` — then fast-forward the base to the
/// result. `base` is the session's recorded base; [`land_base`] resolves the
/// fallback when it is absent or stale. A base that already contains every
/// commit reports `AlreadyLanded` — a land's Resolve-in-chat agent finishing
/// the fast-forward itself lands here on the next run. A stopped
/// integration reports `Conflict` and leaves the rebase or merge in
/// progress; re-running while stopped reports the same conflict again
/// rather than erroring.
pub fn land(cwd: &Path, base: Option<&str>, strategy: PullStrategy) -> anyhow::Result<LandOutcome> {
    ensure_repository(cwd)?;
    let Some(base) = land_base(cwd, base)? else {
        bail!("could not find a base branch to land on");
    };
    if let Some(in_progress) = sync_in_progress(cwd)? {
        return Ok(LandOutcome::Conflict {
            base,
            in_progress,
            files: conflicted_paths(cwd)?,
        });
    }
    if is_ancestor(cwd, "HEAD", &base)? {
        return Ok(LandOutcome::AlreadyLanded { base });
    }
    if !is_ancestor(cwd, &base, "HEAD")? {
        // Diverged history integrates first. Rebase and merge both require a
        // clean tree; refuse rather than autostash, whose pop conflicts land
        // after the integration's own machinery is gone.
        if has_tracked_changes(cwd)? {
            bail!("commit or stash your changes before landing");
        }
        let output = match strategy {
            PullStrategy::Rebase => git_capture(cwd, &["rebase", &base])?,
            PullStrategy::Merge => git_capture(cwd, &["merge", "--no-edit", &base])?,
        };
        if !output.status.success() {
            if let Some(in_progress) = sync_in_progress(cwd)? {
                return Ok(LandOutcome::Conflict {
                    base,
                    in_progress,
                    files: conflicted_paths(cwd)?,
                });
            }
            bail!("{}", command_error(&output));
        }
    }
    // The range must resolve before the base moves: after the fast-forward
    // `base..HEAD` is empty. These are the commits that land, newest first.
    let range = format!("{base}..HEAD");
    let ahead = git_optional_stdout(cwd, &["rev-list", "--count", &range])?
        .and_then(|count| count.trim().parse::<u64>().ok())
        .unwrap_or(0);
    let commits = log_commits(cwd, &[range], 0, LANDED_COMMITS_LIMIT)?;
    fast_forward_base(cwd, &base)?;
    Ok(LandOutcome::Landed {
        base,
        commits,
        ahead,
    })
}

/// The branch a land rebases onto and fast-forwards: the recorded base while
/// it still exists as a local branch, then the remote's default
/// (`origin/HEAD`), then the branch the primary checkout holds, then
/// `main`/`master`. The checkout's own branch never qualifies — being on the
/// base means there is nothing to land.
fn land_base(cwd: &Path, recorded: Option<&str>) -> anyhow::Result<Option<String>> {
    let current = git_optional_stdout(cwd, &["branch", "--show-current"])?
        .filter(|branch| !branch.is_empty());
    let usable = |candidate: Option<String>| -> anyhow::Result<Option<String>> {
        let Some(name) = candidate else {
            return Ok(None);
        };
        if current.as_deref() == Some(name.as_str())
            || !ref_exists(cwd, &format!("refs/heads/{name}"))?
        {
            return Ok(None);
        }
        Ok(Some(name))
    };
    if let Some(base) = usable(recorded.map(str::to_owned))? {
        return Ok(Some(base));
    }
    let remote_default = git_optional_stdout(
        cwd,
        &[
            "symbolic-ref",
            "--quiet",
            "--short",
            "refs/remotes/origin/HEAD",
        ],
    )?
    .and_then(|name| name.strip_prefix("origin/").map(str::to_owned));
    if let Some(base) = usable(remote_default)? {
        return Ok(Some(base));
    }
    if let Some(base) = usable(
        checkouts(cwd)?
            .into_iter()
            .next()
            .and_then(|entry| entry.branch),
    )? {
        return Ok(Some(base));
    }
    for candidate in ["main", "master"] {
        if let Some(base) = usable(Some(candidate.to_owned()))? {
            return Ok(Some(base));
        }
    }
    Ok(None)
}

fn land_target_on_base(cwd: &Path, base: String) -> anyhow::Result<Option<LandTarget>> {
    let ahead = git_optional_stdout(cwd, &["rev-list", "--count", &format!("{base}..HEAD")])?
        .and_then(|count| count.parse::<u64>().ok())
        .unwrap_or(0);
    Ok((ahead > 0).then_some(LandTarget {
        branch: base,
        ahead,
    }))
}

/// Move `base` to HEAD. Git refuses a raw ref move on a checked-out branch,
/// so a base another worktree owns gets a real `--ff-only` merge there — its
/// index and files advance too. A base checked out nowhere moves with
/// `branch -f`; HEAD is already known to contain it.
fn fast_forward_base(cwd: &Path, base: &str) -> anyhow::Result<()> {
    let head = git_stdout(cwd, &["rev-parse", "HEAD"])?;
    let checkout = checkouts(cwd)?
        .into_iter()
        .find(|entry| entry.branch.as_deref() == Some(base))
        .map(|entry| entry.path);
    match checkout {
        Some(path) => {
            git_success(&path, &["merge", "--ff-only", &head])?;
        }
        None => {
            git_success(cwd, &["branch", "-f", base, &head])?;
        }
    }
    Ok(())
}

/// `git merge-base --is-ancestor`: true when `ancestor` is reachable from
/// `descendant`, including when they name the same commit.
fn is_ancestor(cwd: &Path, ancestor: &str, descendant: &str) -> anyhow::Result<bool> {
    Ok(
        git_capture(cwd, &["merge-base", "--is-ancestor", ancestor, descendant])?
            .status
            .success(),
    )
}

/// Staged or unstaged changes to tracked files — what `git rebase` refuses
/// to run on. Untracked files never block it.
fn has_tracked_changes(cwd: &Path) -> anyhow::Result<bool> {
    Ok(!git_stdout(cwd, &["status", "--porcelain=v1", "--untracked-files=no"])?.is_empty())
}

/// One `git worktree list --porcelain` entry: the checkout's path and the
/// branch it holds — `None` for a detached or bare worktree.
struct Checkout {
    path: PathBuf,
    branch: Option<String>,
}

/// Every checkout Git knows about; the primary worktree always leads.
fn checkouts(cwd: &Path) -> anyhow::Result<Vec<Checkout>> {
    let list = git_stdout(cwd, &["worktree", "list", "--porcelain"])?;
    let mut entries = Vec::new();
    let mut path = None;
    for line in list.lines() {
        if let Some(worktree) = line.strip_prefix("worktree ") {
            path = Some(PathBuf::from(worktree));
        } else if let Some(reference) = line.strip_prefix("branch refs/heads/") {
            if let Some(path) = path.take() {
                entries.push(Checkout {
                    path,
                    branch: Some(reference.to_owned()),
                });
            }
        } else if (line == "detached" || line == "bare")
            && let Some(path) = path.take()
        {
            entries.push(Checkout { path, branch: None });
        }
    }
    Ok(entries)
}

/// One commit's metadata for a transcript reference or diff modal. `sha` may
/// be any revision Git resolves to a commit, including an abbreviated hash.
pub fn commit(cwd: &Path, sha: &str) -> anyhow::Result<CommitEntry> {
    ensure_repository(cwd)?;
    let output = git_stdout(
        cwd,
        &[
            "show",
            "-s",
            "--format=%H%x1f%h%x1f%an%x1f%ae%x1f%at%x1f%s%x1f%b",
            sha,
        ],
    )?;
    let mut fields = output.split('\x1f');
    let full_sha = fields.next().unwrap_or_default().trim().to_owned();
    if full_sha.is_empty() {
        bail!("unknown commit {sha}");
    }
    let (additions, deletions) = commit_numstat(cwd, sha)?;
    Ok(CommitEntry {
        sha: full_sha,
        short_sha: fields.next().unwrap_or_default().to_owned(),
        author: fields.next().unwrap_or_default().to_owned(),
        author_email: fields.next().unwrap_or_default().to_owned(),
        authored_at: fields
            .next()
            .and_then(|value| value.trim().parse::<u64>().ok())
            .unwrap_or(0),
        subject: fields.next().unwrap_or_default().to_owned(),
        body: fields.next().unwrap_or_default().trim().to_owned(),
        additions,
        deletions,
    })
}

/// `git log` on HEAD, paged. A repository with no commits yet reads as an
/// empty history rather than an error.
pub fn commits(cwd: &Path, skip: usize, limit: usize) -> anyhow::Result<Vec<CommitEntry>> {
    ensure_repository(cwd)?;
    if !ref_exists(cwd, "HEAD")? {
        return Ok(Vec::new());
    }
    log_commits(cwd, &[], skip, limit)
}

/// `git log <upstream> --not HEAD`: the commits the tracking branch has that
/// the checkout lacks — what a pull would bring in. Empty when the branch
/// has no upstream.
pub fn upstream_commits(cwd: &Path, skip: usize, limit: usize) -> anyhow::Result<Vec<CommitEntry>> {
    ensure_repository(cwd)?;
    let Some(upstream) = upstream(cwd)? else {
        return Ok(Vec::new());
    };
    let revs = if ref_exists(cwd, "HEAD")? {
        vec![upstream, "--not".to_owned(), "HEAD".to_owned()]
    } else {
        vec![upstream]
    };
    log_commits(cwd, &revs, skip, limit)
}

fn log_commits(
    cwd: &Path,
    revs: &[String],
    skip: usize,
    limit: usize,
) -> anyhow::Result<Vec<CommitEntry>> {
    // \x1f separates fields, \x1e records; both are illegal in commit
    // subjects and all but impossible in bodies.
    let mut args = vec![
        "log".to_owned(),
        format!("--skip={skip}"),
        format!("-n{limit}"),
        "--format=%H%x1f%h%x1f%an%x1f%ae%x1f%at%x1f%s%x1f%b%x1e".to_owned(),
    ];
    args.extend(revs.iter().cloned());
    let output = git_stdout(cwd, &args.iter().map(String::as_str).collect::<Vec<_>>())?;
    let stats = commit_numstats(cwd, revs, skip, limit)?;
    Ok(output
        .split('\x1e')
        .filter_map(|record| {
            let mut fields = record.split('\x1f');
            let sha = fields.next()?.trim().to_owned();
            let short_sha = fields.next()?.to_owned();
            let author = fields.next()?.to_owned();
            let author_email = fields.next()?.to_owned();
            let authored_at = fields
                .next()
                .and_then(|value| value.trim().parse::<u64>().ok())
                .unwrap_or(0);
            let subject = fields.next()?.to_owned();
            let body = fields.next().unwrap_or_default().trim().to_owned();
            if sha.is_empty() {
                return None;
            }
            let (additions, deletions) = stats.get(&sha).copied().unwrap_or_default();
            Some(CommitEntry {
                sha,
                short_sha,
                subject,
                body,
                author,
                author_email,
                authored_at,
                additions,
                deletions,
            })
        })
        .collect())
}

/// One commit's `+/-` totals. `--numstat` reports `-\t-` for binary files,
/// which simply contribute no line counts here, matching the commit list.
fn commit_numstat(cwd: &Path, sha: &str) -> anyhow::Result<(u64, u64)> {
    let output = git_stdout(cwd, &["show", "--numstat", "--format=", sha])?;
    Ok(output.lines().fold((0, 0), |(additions, deletions), line| {
        let mut fields = line.split('\t');
        (
            additions
                + fields
                    .next()
                    .and_then(|value| value.parse::<u64>().ok())
                    .unwrap_or(0),
            deletions
                + fields
                    .next()
                    .and_then(|value| value.parse::<u64>().ok())
                    .unwrap_or(0),
        )
    }))
}

/// Per-commit `+/-` totals for the same `git log` page, keyed by full sha.
/// `--numstat` prints each commit's `add\tdel\tpath` lines after its format
/// record; the leading `\x1e` keeps the two aligned when the diff is empty
/// (merges, binary-only changes), which `--numstat` simply omits lines for.
fn commit_numstats(
    cwd: &Path,
    revs: &[String],
    skip: usize,
    limit: usize,
) -> anyhow::Result<HashMap<String, (u64, u64)>> {
    let mut args = vec![
        "log".to_owned(),
        format!("--skip={skip}"),
        format!("-n{limit}"),
        "--format=%x1e%H".to_owned(),
        "--numstat".to_owned(),
    ];
    args.extend(revs.iter().cloned());
    let output = git_stdout(cwd, &args.iter().map(String::as_str).collect::<Vec<_>>())?;
    let mut stats = HashMap::new();
    for record in output.split('\x1e') {
        let mut lines = record.lines();
        let Some(sha) = lines.next().map(str::trim).filter(|sha| !sha.is_empty()) else {
            continue;
        };
        let totals = lines.fold((0u64, 0u64), |(additions, deletions), line| {
            let mut fields = line.splitn(3, '\t');
            match (
                fields.next().and_then(|value| value.parse::<u64>().ok()),
                fields.next().and_then(|value| value.parse::<u64>().ok()),
            ) {
                (Some(added), Some(deleted)) => (additions + added, deletions + deleted),
                _ => (additions, deletions),
            }
        });
        stats.insert(sha.to_owned(), totals);
    }
    Ok(stats)
}

/// `rev-list --left-right --count HEAD...<upstream>`: commits each side has
/// that the other lacks.
fn ahead_behind(cwd: &Path, upstream: &str) -> anyhow::Result<(u64, u64)> {
    let output = git_stdout(
        cwd,
        &[
            "rev-list",
            "--count",
            "--left-right",
            &format!("HEAD...{upstream}"),
        ],
    )?;
    let mut sides = output.split('\t');
    let ahead = sides
        .next()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .unwrap_or(0);
    let behind = sides
        .next()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .unwrap_or(0);
    Ok((ahead, behind))
}

/// Whether `rev-parse --verify` resolves the ref.
fn ref_exists(cwd: &Path, reference: &str) -> anyhow::Result<bool> {
    let output = git_capture(cwd, &["rev-parse", "--verify", "--quiet", reference])
        .context("failed to resolve Git ref")?;
    Ok(output.status.success())
}

/// Per-path `additions/deletions` from a `--numstat -z` diff. With `-z` each
/// record is `add\tdel\tpath\0`; a rename leaves the path empty and follows
/// with `old\0new\0`, and the postimage name keys the map.
fn numstat_map(cwd: &Path, args: &[&str]) -> anyhow::Result<HashMap<String, (u64, u64)>> {
    let output = git_stdout(cwd, args)?;
    let mut map = HashMap::new();
    let mut fields = output.split('\0').filter(|field| !field.is_empty());
    while let Some(field) = fields.next() {
        let mut parts = field.splitn(3, '\t');
        let additions = parts.next().and_then(|value| value.parse::<u64>().ok());
        let deletions = parts.next().and_then(|value| value.parse::<u64>().ok());
        let path = match parts.next() {
            Some(path) => path.to_owned(),
            None => continue,
        };
        // A rename's path field is empty; the next two fields carry the old
        // then new path, and the change belongs to the new one.
        let path = if path.is_empty() {
            let Some(_old) = fields.next() else { break };
            let Some(new) = fields.next() else { break };
            new.to_owned()
        } else {
            path
        };
        map.insert(path, (additions.unwrap_or(0), deletions.unwrap_or(0)));
    }
    Ok(map)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::process::Command;

    use super::*;

    fn run_git(cwd: &Path, args: &[&str]) {
        let status = Command::new("git")
            .args(args)
            .current_dir(cwd)
            .status()
            .expect("git should run");
        assert!(status.success(), "git {:?} failed", args);
    }

    fn repository() -> PathBuf {
        let directory =
            std::env::temp_dir().join(format!("waku-git-panel-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        run_git(&directory, &["init", "-q"]);
        run_git(&directory, &["config", "user.email", "test@example.com"]);
        run_git(&directory, &["config", "user.name", "Test"]);
        directory
    }

    #[test]
    fn inspect_splits_staged_and_unstaged_halves_of_one_file() {
        let cwd = repository();
        std::fs::write(cwd.join("file.txt"), "one\n").unwrap();
        run_git(&cwd, &["add", "file.txt"]);
        run_git(&cwd, &["commit", "-qm", "init"]);
        std::fs::write(cwd.join("file.txt"), "one\ntwo\n").unwrap();
        run_git(&cwd, &["add", "file.txt"]);
        std::fs::write(cwd.join("file.txt"), "one\ntwo\nthree\n").unwrap();

        let snapshot = inspect(&cwd, None).unwrap().unwrap();
        assert_eq!(snapshot.staged.len(), 1);
        assert_eq!(snapshot.unstaged.len(), 1);
        assert_eq!(snapshot.staged[0].path, "file.txt");
        assert_eq!(snapshot.staged[0].additions, 1);
        assert_eq!(snapshot.unstaged[0].path, "file.txt");
        assert_eq!(snapshot.unstaged[0].additions, 1);
    }

    #[test]
    fn inspect_lists_untracked_files_without_counts() {
        let cwd = repository();
        run_git(&cwd, &["commit", "-qm", "init", "--allow-empty"]);
        std::fs::write(cwd.join("new.txt"), "hello\n").unwrap();

        let snapshot = inspect(&cwd, None).unwrap().unwrap();
        assert_eq!(snapshot.unstaged.len(), 1);
        assert!(snapshot.unstaged[0].untracked);
        assert_eq!(snapshot.unstaged[0].status, "??");
    }

    #[test]
    fn stage_and_unstage_move_a_file_between_lists() {
        let cwd = repository();
        run_git(&cwd, &["commit", "-qm", "init", "--allow-empty"]);
        std::fs::write(cwd.join("file.txt"), "hello\n").unwrap();

        stage(&cwd, "file.txt").unwrap();
        let snapshot = inspect(&cwd, None).unwrap().unwrap();
        assert_eq!(snapshot.staged.len(), 1);
        assert!(snapshot.unstaged.is_empty());

        // No HEAD yet: unstage falls back to `git rm --cached`.
        unstage(&cwd, "file.txt").unwrap();
        let snapshot = inspect(&cwd, None).unwrap().unwrap();
        assert!(snapshot.staged.is_empty());
        assert!(snapshot.unstaged[0].untracked);
    }

    #[test]
    fn unstage_uses_reset_once_head_exists() {
        let cwd = repository();
        std::fs::write(cwd.join("file.txt"), "one\n").unwrap();
        run_git(&cwd, &["add", "file.txt"]);
        run_git(&cwd, &["commit", "-qm", "init"]);
        std::fs::write(cwd.join("file.txt"), "one\ntwo\n").unwrap();
        run_git(&cwd, &["add", "file.txt"]);

        unstage(&cwd, "file.txt").unwrap();
        let snapshot = inspect(&cwd, None).unwrap().unwrap();
        assert!(snapshot.staged.is_empty());
        assert_eq!(snapshot.unstaged.len(), 1);
        assert!(!snapshot.unstaged[0].untracked);
    }

    #[test]
    fn commits_page_through_log() {
        let cwd = repository();
        for index in 0..5 {
            run_git(
                &cwd,
                &["commit", "-qm", &format!("commit {index}"), "--allow-empty"],
            );
        }
        let page = commits(&cwd, 1, 2).unwrap();
        assert_eq!(page.len(), 2);
        assert_eq!(page[0].subject, "commit 3");
        assert_eq!(page[1].subject, "commit 2");
        assert_eq!(page[0].short_sha.len(), 7);
        assert_eq!(page[0].author, "Test");
        assert_eq!(page[0].author_email, "test@example.com");
        assert!(page[0].authored_at > 0);
        assert_eq!((page[0].additions, page[0].deletions), (0, 0));
    }

    #[test]
    fn commits_carry_numstat_totals() {
        let cwd = repository();
        std::fs::write(cwd.join("file.txt"), "one\ntwo\n").unwrap();
        run_git(&cwd, &["add", "file.txt"]);
        run_git(&cwd, &["commit", "-qm", "init"]);
        std::fs::write(cwd.join("file.txt"), "one\n").unwrap();
        run_git(&cwd, &["commit", "-qam", "drop a line"]);

        let page = commits(&cwd, 0, 10).unwrap();
        assert_eq!(page[0].subject, "drop a line");
        assert_eq!((page[0].additions, page[0].deletions), (0, 1));
        assert_eq!((page[1].additions, page[1].deletions), (2, 0));
    }

    #[test]
    fn commits_is_empty_before_the_first_commit() {
        let cwd = repository();
        assert!(commits(&cwd, 0, 10).unwrap().is_empty());
    }

    #[test]
    fn commit_resolves_full_and_abbreviated_shas() {
        let cwd = repository();
        std::fs::write(cwd.join("file.txt"), "one\ntwo\n").unwrap();
        run_git(&cwd, &["add", "file.txt"]);
        run_git(&cwd, &["commit", "-qm", "init"]);
        let listed = commits(&cwd, 0, 1).unwrap().remove(0);

        for sha in [&listed.sha, &listed.short_sha] {
            let entry = commit(&cwd, sha).unwrap();
            assert_eq!(entry.sha, listed.sha);
            assert_eq!(entry.subject, "init");
            assert_eq!(entry.author, "Test");
            assert_eq!((entry.additions, entry.deletions), (2, 0));
        }
    }

    /// A repository whose primary checkout is on `main` plus a linked
    /// worktree detached at the initial commit — the shape a task session
    /// lands from.
    fn land_repository() -> (PathBuf, PathBuf, PathBuf) {
        let root =
            std::env::temp_dir().join(format!("waku-git-panel-test-{}", uuid::Uuid::new_v4()));
        let repository = root.join("primary");
        std::fs::create_dir_all(&repository).unwrap();
        run_git(&repository, &["init", "-q", "-b", "main"]);
        run_git(&repository, &["config", "user.email", "test@example.com"]);
        run_git(&repository, &["config", "user.name", "Test"]);
        std::fs::write(repository.join("file.txt"), "base\n").unwrap();
        run_git(&repository, &["add", "file.txt"]);
        run_git(&repository, &["commit", "-qm", "init"]);
        let worktree = root.join("session");
        run_git(
            &repository,
            &[
                "worktree",
                "add",
                "-q",
                "--detach",
                worktree.to_str().unwrap(),
            ],
        );
        (root, repository, worktree)
    }

    /// One commit on top of the worktree's HEAD.
    fn commit_in(cwd: &Path, name: &str, contents: &str) {
        std::fs::write(cwd.join(name), contents).unwrap();
        run_git(cwd, &["add", name]);
        run_git(cwd, &["commit", "-qm", "session work"]);
    }

    #[test]
    fn land_fast_forwards_the_base_inside_its_checkout() {
        let (root, repository, worktree) = land_repository();
        commit_in(&worktree, "work.txt", "session\n");

        let outcome = land(&worktree, Some("main"), PullStrategy::Rebase).unwrap();
        let LandOutcome::Landed {
            base,
            commits,
            ahead,
        } = outcome
        else {
            panic!("expected Landed, got {outcome:?}");
        };
        assert_eq!(base, "main");
        assert_eq!(ahead, 1);
        assert_eq!(commits.len(), 1);
        assert_eq!(commits[0].subject, "session work");
        // The merge ran inside the primary checkout: its files moved too.
        assert!(repository.join("work.txt").exists());
        assert_eq!(
            git_stdout(&repository, &["rev-parse", "main"]).unwrap(),
            git_stdout(&worktree, &["rev-parse", "HEAD"]).unwrap()
        );
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn land_rebases_a_diverged_worktree_before_landing() {
        let (root, repository, worktree) = land_repository();
        std::fs::write(repository.join("base.txt"), "new base work\n").unwrap();
        run_git(&repository, &["add", "base.txt"]);
        run_git(&repository, &["commit", "-qm", "base advances"]);
        commit_in(&worktree, "work.txt", "session\n");

        let outcome = land(&worktree, Some("main"), PullStrategy::Rebase).unwrap();
        let LandOutcome::Landed { commits, ahead, .. } = outcome else {
            panic!("expected Landed, got {outcome:?}");
        };
        assert_eq!(ahead, 1);
        assert_eq!(commits.len(), 1);
        assert!(worktree.join("base.txt").exists());
        assert_eq!(
            git_stdout(&repository, &["rev-parse", "main"]).unwrap(),
            git_stdout(&worktree, &["rev-parse", "HEAD"]).unwrap()
        );
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn land_reports_the_stopped_rebase_and_recovers_on_abort() {
        let (root, repository, worktree) = land_repository();
        std::fs::write(repository.join("file.txt"), "base changed\n").unwrap();
        run_git(&repository, &["commit", "-qam", "base changes"]);
        std::fs::write(worktree.join("file.txt"), "session changed\n").unwrap();
        run_git(&worktree, &["commit", "-qam", "session changes"]);

        let outcome = land(&worktree, Some("main"), PullStrategy::Rebase).unwrap();
        assert_eq!(
            outcome,
            LandOutcome::Conflict {
                base: "main".to_owned(),
                in_progress: SyncInProgress::Rebase,
                files: vec!["file.txt".to_owned()],
            }
        );
        // Re-running while stopped reports the conflict again.
        let outcome = land(&worktree, Some("main"), PullStrategy::Rebase).unwrap();
        assert!(matches!(outcome, LandOutcome::Conflict { .. }));

        abort_sync(&worktree).unwrap();
        assert_eq!(
            git_stdout(&worktree, &["status", "--porcelain"]).unwrap(),
            ""
        );
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn land_after_a_continued_rebase_lands_instead_of_reconflicting() {
        let (root, repository, worktree) = land_repository();
        std::fs::write(repository.join("file.txt"), "base changed\n").unwrap();
        run_git(&repository, &["commit", "-qam", "base changes"]);
        std::fs::write(worktree.join("file.txt"), "session changed\n").unwrap();
        run_git(&worktree, &["commit", "-qam", "session changes"]);

        let outcome = land(&worktree, Some("main"), PullStrategy::Rebase).unwrap();
        assert!(matches!(outcome, LandOutcome::Conflict { .. }));

        // The Resolve in chat path: the agent resolves, continues, and the
        // rebase finishes — but REBASE_HEAD still resolves.
        std::fs::write(worktree.join("file.txt"), "resolved\n").unwrap();
        run_git(&worktree, &["add", "file.txt"]);
        run_git(&worktree, &["-c", "core.editor=true", "rebase", "--continue"]);
        assert!(ref_exists(&worktree, "REBASE_HEAD").unwrap());

        let outcome = land(&worktree, Some("main"), PullStrategy::Rebase).unwrap();
        let LandOutcome::Landed { commits, ahead, .. } = outcome else {
            panic!("expected Landed, got {outcome:?}");
        };
        assert_eq!(ahead, 1);
        assert_eq!(commits.len(), 1);
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn land_merge_instead_lands_as_a_merge_commit() {
        let (root, repository, worktree) = land_repository();
        std::fs::write(repository.join("base.txt"), "new base work\n").unwrap();
        run_git(&repository, &["add", "base.txt"]);
        run_git(&repository, &["commit", "-qm", "base advances"]);
        commit_in(&worktree, "work.txt", "session\n");

        let outcome = land(&worktree, Some("main"), PullStrategy::Merge).unwrap();
        let LandOutcome::Landed { commits, ahead, .. } = outcome else {
            panic!("expected Landed, got {outcome:?}");
        };
        // The merge commit plus the worktree's own commit land together.
        assert_eq!(ahead, 2);
        assert_eq!(commits.len(), 2);
        // The base fast-forwarded to a merge commit: two parents.
        git_stdout(&repository, &["rev-parse", "--verify", "main^2"]).unwrap();
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn land_moves_the_ref_when_the_base_is_not_checked_out() {
        let (root, repository, worktree) = land_repository();
        run_git(&repository, &["switch", "-q", "-c", "other"]);
        commit_in(&worktree, "work.txt", "session\n");

        let outcome = land(&worktree, Some("main"), PullStrategy::Rebase).unwrap();
        assert!(matches!(outcome, LandOutcome::Landed { base, .. } if base == "main"));
        assert_eq!(
            git_stdout(&repository, &["rev-parse", "main"]).unwrap(),
            git_stdout(&worktree, &["rev-parse", "HEAD"]).unwrap()
        );
        // The primary checkout stayed on `other`; no files moved there.
        assert_eq!(
            git_stdout(&repository, &["branch", "--show-current"]).unwrap(),
            "other"
        );
        assert!(!repository.join("work.txt").exists());
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn land_refuses_a_dirty_diverged_checkout() {
        let (root, repository, worktree) = land_repository();
        std::fs::write(repository.join("base.txt"), "new base work\n").unwrap();
        run_git(&repository, &["add", "base.txt"]);
        run_git(&repository, &["commit", "-qm", "base advances"]);
        commit_in(&worktree, "work.txt", "session\n");
        std::fs::write(worktree.join("file.txt"), "uncommitted\n").unwrap();

        assert!(land(&worktree, Some("main"), PullStrategy::Rebase).is_err());
        // Untracked files never block: dropping the tracked change lands.
        run_git(&worktree, &["checkout", "--", "file.txt"]);
        std::fs::write(worktree.join("scratch.txt"), "untracked\n").unwrap();
        assert!(matches!(
            land(&worktree, Some("main"), PullStrategy::Rebase).unwrap(),
            LandOutcome::Landed { .. }
        ));
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn land_reports_already_landed_when_there_is_nothing_to_land() {
        let (root, _repository, worktree) = land_repository();
        assert_eq!(
            land(&worktree, Some("main"), PullStrategy::Rebase).unwrap(),
            LandOutcome::AlreadyLanded {
                base: "main".to_owned()
            }
        );
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn inspect_reports_the_land_target_only_when_ahead() {
        let (root, repository, worktree) = land_repository();
        // The primary checkout is on the base itself — no target.
        assert_eq!(
            inspect(&repository, None).unwrap().unwrap().land_target,
            None
        );
        commit_in(&worktree, "work.txt", "session\n");
        let snapshot = inspect(&worktree, Some("main")).unwrap().unwrap();
        assert_eq!(
            snapshot.land_target,
            Some(LandTarget {
                branch: "main".to_owned(),
                ahead: 1,
            })
        );
        std::fs::remove_dir_all(root).ok();
    }
}
