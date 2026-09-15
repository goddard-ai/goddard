//! The Git panel's working-tree reads and mutations.
//!
//! Everything here runs on the daemon host inside a workspace request; the
//! panel's render path only ever sees the returned snapshots.

use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context as _, bail};

use waku_protocol::git::{
    CommitEntry, GitFileChange, GitPanelSnapshot, PullOutcome, PullStrategy, SyncInProgress,
    UpstreamStatus,
};

use crate::git_commit::{
    command_error, ensure_repository, git_capture, git_optional_stdout, git_stdout, git_success,
    push_target, remote_for_branch, upstream,
};

/// The panel's one-shot state read. `Ok(None)` outside a work tree.
pub fn inspect(cwd: &Path) -> anyhow::Result<Option<GitPanelSnapshot>> {
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
    Ok(Some(GitPanelSnapshot {
        branch,
        upstream: upstream_status,
        can_push,
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
        return Ok(PullOutcome::Conflict { in_progress });
    }
    bail!("{}", command_error(&output))
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

/// Whether a rebase or merge stopped on conflicts. `REBASE_HEAD` only exists
/// while a rebase is in progress; `MERGE_HEAD` likewise for merges.
fn sync_in_progress(cwd: &Path) -> anyhow::Result<Option<SyncInProgress>> {
    if ref_exists(cwd, "REBASE_HEAD")? {
        return Ok(Some(SyncInProgress::Rebase));
    }
    if ref_exists(cwd, "MERGE_HEAD")? {
        return Ok(Some(SyncInProgress::Merge));
    }
    Ok(None)
}

/// `git log` on HEAD, paged. A repository with no commits yet reads as an
/// empty history rather than an error.
pub fn commits(cwd: &Path, skip: usize, limit: usize) -> anyhow::Result<Vec<CommitEntry>> {
    ensure_repository(cwd)?;
    if !ref_exists(cwd, "HEAD")? {
        return Ok(Vec::new());
    }
    // \x1f separates fields, \x1e records; both are illegal in commit
    // subjects and all but impossible in bodies.
    let output = git_stdout(
        cwd,
        &[
            "log",
            &format!("--skip={skip}"),
            &format!("-n{limit}"),
            "--format=%H%x1f%h%x1f%an%x1f%at%x1f%s%x1f%b%x1e",
        ],
    )?;
    let stats = commit_numstats(cwd, skip, limit)?;
    Ok(output
        .split('\x1e')
        .filter_map(|record| {
            let mut fields = record.split('\x1f');
            let sha = fields.next()?.trim().to_owned();
            let short_sha = fields.next()?.to_owned();
            let author = fields.next()?.to_owned();
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
                authored_at,
                additions,
                deletions,
            })
        })
        .collect())
}

/// Per-commit `+/-` totals for the same `git log` page, keyed by full sha.
/// `--numstat` prints each commit's `add\tdel\tpath` lines after its format
/// record; the leading `\x1e` keeps the two aligned when the diff is empty
/// (merges, binary-only changes), which `--numstat` simply omits lines for.
fn commit_numstats(
    cwd: &Path,
    skip: usize,
    limit: usize,
) -> anyhow::Result<HashMap<String, (u64, u64)>> {
    let output = git_stdout(
        cwd,
        &[
            "log",
            &format!("--skip={skip}"),
            &format!("-n{limit}"),
            "--format=%x1e%H",
            "--numstat",
        ],
    )?;
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

        let snapshot = inspect(&cwd).unwrap().unwrap();
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

        let snapshot = inspect(&cwd).unwrap().unwrap();
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
        let snapshot = inspect(&cwd).unwrap().unwrap();
        assert_eq!(snapshot.staged.len(), 1);
        assert!(snapshot.unstaged.is_empty());

        // No HEAD yet: unstage falls back to `git rm --cached`.
        unstage(&cwd, "file.txt").unwrap();
        let snapshot = inspect(&cwd).unwrap().unwrap();
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
        let snapshot = inspect(&cwd).unwrap().unwrap();
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
}
