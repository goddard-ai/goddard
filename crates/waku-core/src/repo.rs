//! Daemon-owned repository-level Git reads for the Projects page.
//!
//! Every function in this module performs process I/O. Callers must run them
//! from the background executor; render paths consume only the values they
//! return. The list reads follow the `pull_requests` contract: `None` means
//! `cwd` is not inside a Git repository, which callers render as "unknown",
//! distinct from an empty `Some`.

use std::path::{Path, PathBuf};
use std::process::Output;

use anyhow::{Context as _, bail};

pub use waku_protocol::workspace::{BranchDeleteFailure, RepoBranch, RepoWorktree};

/// The repository's worktrees — the ordinary checkout first, then linked
/// ones — enriched with per-checkout status. `Ok(None)` means `cwd` is not
/// inside a Git repository. A read that fails for one worktree degrades that
/// entry's optional fields to `None` rather than failing the whole list.
pub fn list_worktrees(cwd: &Path) -> anyhow::Result<Option<Vec<RepoWorktree>>> {
    // `worktree list` resolves the repository from anywhere inside it,
    // including a linked worktree, and reports the main checkout first.
    let output = crate::command_env::plain_command("git")
        .args(["worktree", "list", "--porcelain"])
        .current_dir(cwd)
        .output()
        .context("failed to execute git")?;
    if !output.status.success() {
        return Ok(None);
    }
    let entries = parse_worktree_porcelain(&String::from_utf8_lossy(&output.stdout))
        .into_iter()
        .enumerate()
        .map(|(index, parsed)| {
            let mut entry = RepoWorktree {
                path: parsed.path,
                head: parsed.head,
                branch: parsed.branch,
                // The first block is always the main working tree.
                is_main: index == 0,
                dirty_files: None,
                ahead: None,
                behind: None,
                last_commit_at: None,
            };
            // A bare repository has no working tree to take the status of.
            if !parsed.bare {
                enrich_worktree(&mut entry);
            }
            entry
        })
        .collect();
    Ok(Some(entries))
}

/// One `worktree list --porcelain` block: the attributes between a
/// `worktree <path>` line and the blank line that ends the block.
struct ParsedWorktree {
    path: PathBuf,
    head: String,
    branch: Option<String>,
    bare: bool,
}

fn parse_worktree_porcelain(output: &str) -> Vec<ParsedWorktree> {
    let mut worktrees = Vec::new();
    let mut current: Option<ParsedWorktree> = None;
    for line in output.lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            if let Some(worktree) = current.take() {
                worktrees.push(worktree);
            }
            current = Some(ParsedWorktree {
                path: PathBuf::from(path.trim()),
                head: String::new(),
                branch: None,
                bare: false,
            });
            continue;
        }
        let Some(worktree) = current.as_mut() else {
            continue;
        };
        if let Some(head) = line.strip_prefix("HEAD ") {
            worktree.head = head.trim().to_owned();
        } else if let Some(reference) = line.strip_prefix("branch ") {
            worktree.branch = Some(
                reference
                    .trim()
                    .strip_prefix("refs/heads/")
                    .unwrap_or(reference.trim())
                    .to_owned(),
            );
        } else if line.trim() == "bare" {
            worktree.bare = true;
        }
        // `detached`, `prunable`, and `locked` need no state of their own.
    }
    if let Some(worktree) = current.take() {
        worktrees.push(worktree);
    }
    worktrees
}

/// Fill one entry's status fields from two `git -C <path>` reads: a single
/// `status --porcelain=v1 --branch` gives both the dirty count and the
/// upstream divergence, and `log -1` gives HEAD's commit time. Either read
/// failing — a deleted directory, an unborn branch — leaves the fields it
/// would have answered at `None`.
fn enrich_worktree(entry: &mut RepoWorktree) {
    if let Some(status) = worktree_stdout(&entry.path, &["status", "--porcelain=v1", "--branch"]) {
        let mut lines = status.lines();
        let (ahead, behind) = status_tracking(lines.next().unwrap_or_default());
        entry.ahead = ahead;
        entry.behind = behind;
        entry.dirty_files = Some(lines.filter(|line| !line.is_empty()).count() as u64);
    }
    entry.last_commit_at = worktree_stdout(&entry.path, &["log", "-1", "--format=%ct"])
        .and_then(|time| time.parse().ok());
}

/// `git -C <path> <args>` trimmed stdout, `None` on any failure — the
/// per-worktree degradation `enrich_worktree` relies on.
fn worktree_stdout(path: &Path, args: &[&str]) -> Option<String> {
    let output = crate::command_env::plain_command("git")
        .arg("-C")
        .arg(path)
        .args(args)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

/// The `## <branch>...<upstream> [ahead N, behind M]` header's divergence.
/// A header without `...` — a detached HEAD, an unborn branch, or one with
/// no upstream — reports `None`s; an upstream with no bracket is even; a
/// `[gone]` upstream cannot be measured.
fn status_tracking(header: &str) -> (Option<u64>, Option<u64>) {
    if !header.contains("...") {
        return (None, None);
    }
    let bracket = header
        .split_once('[')
        .and_then(|(_, rest)| rest.split_once(']'))
        .map(|(inside, _)| inside);
    let Some(bracket) = bracket else {
        return (Some(0), Some(0));
    };
    let mut ahead = None;
    let mut behind = None;
    for token in bracket.split(',') {
        let token = token.trim();
        if let Some(count) = token.strip_prefix("ahead ") {
            ahead = count.parse::<u64>().ok();
        } else if let Some(count) = token.strip_prefix("behind ") {
            behind = count.parse::<u64>().ok();
        }
    }
    if ahead.is_none() && behind.is_none() {
        (None, None)
    } else {
        (Some(ahead.unwrap_or(0)), Some(behind.unwrap_or(0)))
    }
}

/// Local branches and remote-tracking refs for the Projects page's branch
/// table. `Ok(None)` means `cwd` is not inside a Git repository.
pub fn list_repo_branches(cwd: &Path) -> anyhow::Result<Option<Vec<RepoBranch>>> {
    // NUL-separated fields keep commit subjects and worktree paths safe to
    // split; `%(upstream:track)` carries the `[ahead N, behind M]` counts.
    const FORMAT: &str = "%(refname)%00%(objectname)%00%(upstream:short)%00%(upstream:track)%00%(committerdate:unix)%00%(subject)%00%(worktreepath)";
    let output = crate::command_env::plain_command("git")
        .args(["for-each-ref", &format!("--format={FORMAT}")])
        .args(["refs/heads", "refs/remotes"])
        .current_dir(cwd)
        .output()
        .context("failed to execute git")?;
    if !output.status.success() {
        return Ok(None);
    }
    let entries = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(parse_repo_branch)
        .collect();
    Ok(Some(entries))
}

fn parse_repo_branch(line: &str) -> Option<RepoBranch> {
    let mut fields = line.split('\0');
    let refname = fields.next()?;
    let sha = fields.next()?.to_owned();
    let upstream = fields.next()?;
    let track = fields.next()?;
    let committed_at = fields.next()?;
    let subject = fields.next()?;
    let worktree_path = fields.next()?;
    let (name, remote) = if let Some(local) = refname.strip_prefix("refs/heads/") {
        (local.to_owned(), None)
    } else if let Some(remote_ref) = refname.strip_prefix("refs/remotes/") {
        // `refs/remotes/<remote>/<name>`; the remote's own `HEAD` is a
        // default-branch symref, not a branch.
        let (remote, name) = remote_ref.split_once('/')?;
        if name == "HEAD" {
            return None;
        }
        (name.to_owned(), Some(remote.to_owned()))
    } else {
        return None;
    };
    let upstream = (!upstream.is_empty()).then(|| upstream.to_owned());
    let (ahead, behind) = if upstream.is_some() {
        upstream_counts(track)
    } else {
        (None, None)
    };
    Some(RepoBranch {
        name,
        remote,
        sha,
        upstream,
        ahead,
        behind,
        last_commit_at: committed_at.parse().ok(),
        last_commit_subject: (!subject.is_empty()).then(|| subject.to_owned()),
        checked_out_in: (!worktree_path.is_empty()).then(|| PathBuf::from(worktree_path)),
    })
}

/// `%(upstream:track)`'s bracketed counts: `[ahead N, behind M]` — a lone
/// side when the other is zero — `[gone]` when the upstream ref is missing,
/// or empty for a branch even with its upstream.
fn upstream_counts(track: &str) -> (Option<u64>, Option<u64>) {
    let Some(inside) = track
        .strip_prefix('[')
        .and_then(|track| track.strip_suffix(']'))
    else {
        return (Some(0), Some(0));
    };
    let mut ahead = None;
    let mut behind = None;
    for token in inside.split(',') {
        let token = token.trim();
        if let Some(count) = token.strip_prefix("ahead ") {
            ahead = count.parse::<u64>().ok();
        } else if let Some(count) = token.strip_prefix("behind ") {
            behind = count.parse::<u64>().ok();
        }
    }
    if ahead.is_none() && behind.is_none() {
        (None, None)
    } else {
        (Some(ahead.unwrap_or(0)), Some(behind.unwrap_or(0)))
    }
}

/// `git fetch --prune <remote>`: refresh one remote's tracking refs and drop
/// the ones it deleted. Git's own stderr propagates as the error.
pub fn fetch_remote(cwd: &Path, remote: &str) -> anyhow::Result<()> {
    let output = crate::command_env::plain_command("git")
        .args(["fetch", "--prune"])
        .arg(remote)
        .current_dir(cwd)
        .output()
        .context("failed to execute git fetch")?;
    if !output.status.success() {
        bail!("{}", command_error(&output));
    }
    Ok(())
}

/// Delete each local branch — `git branch -d`, or `-D` when `force`. Git's
/// refusals, such as a branch checked out in a worktree or an unmerged
/// branch without `force`, collect per branch rather than failing the
/// batch; an empty return means every requested branch is gone.
pub fn delete_branches(
    cwd: &Path,
    names: &[String],
    force: bool,
) -> anyhow::Result<Vec<BranchDeleteFailure>> {
    let flag = if force { "-D" } else { "-d" };
    let mut failures = Vec::new();
    for name in names {
        let output = crate::command_env::plain_command("git")
            .args(["branch", flag, "--"])
            .arg(name)
            .current_dir(cwd)
            .output()
            .context("failed to execute git branch")?;
        if !output.status.success() {
            failures.push(BranchDeleteFailure {
                name: name.clone(),
                error: command_error(&output),
            });
        }
    }
    Ok(failures)
}

/// `git worktree prune`: drop registrations whose directories were deleted
/// outside the app.
pub fn prune_worktrees(cwd: &Path) -> anyhow::Result<()> {
    let output = crate::command_env::plain_command("git")
        .args(["worktree", "prune"])
        .current_dir(cwd)
        .output()
        .context("failed to execute git worktree prune")?;
    if !output.status.success() {
        bail!("{}", command_error(&output));
    }
    Ok(())
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

    fn run_git(cwd: &Path, args: &[&str]) {
        let output = crate::command_env::plain_command("git")
            .args(args)
            .current_dir(cwd)
            .output()
            .unwrap();
        assert!(output.status.success(), "{}", command_error(&output));
    }

    fn commit(cwd: &Path, message: &str) {
        run_git(cwd, &["add", "."]);
        run_git(
            cwd,
            &[
                "-c",
                "user.name=Goddard Tests",
                "-c",
                "user.email=waku@example.com",
                "commit",
                "-m",
                message,
            ],
        );
    }

    fn repository() -> PathBuf {
        let root = std::env::temp_dir().join(format!("waku-repo-test-{}", Uuid::new_v4()));
        let repository = root.join("repository");
        fs::create_dir_all(&repository).unwrap();
        run_git(&repository, &["init", "-b", "main"]);
        fs::write(repository.join("README.md"), "main\n").unwrap();
        commit(&repository, "initial");
        run_git(&repository, &["branch", "feature"]);
        // `git worktree list` reports canonical paths; match them on macOS,
        // where the temporary directory lives behind `/var` -> `/private/var`.
        fs::canonicalize(&repository).unwrap()
    }

    fn non_repository() -> PathBuf {
        let root = std::env::temp_dir().join(format!("waku-repo-norepo-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn lists_main_and_linked_worktrees_with_status() {
        let repository = repository();
        let linked = repository.with_extension("linked");
        run_git(
            &repository,
            &[
                "worktree",
                "add",
                "-b",
                "occupied",
                linked.to_str().unwrap(),
            ],
        );
        fs::write(repository.join("dirty.txt"), "dirty\n").unwrap();
        fs::write(linked.join("notes.txt"), "notes\n").unwrap();

        let entries = list_worktrees(&repository).unwrap().unwrap();
        assert_eq!(entries.len(), 2);
        let main = &entries[0];
        assert!(main.is_main);
        assert_eq!(main.path, repository);
        assert_eq!(main.branch.as_deref(), Some("main"));
        assert_eq!(main.dirty_files, Some(1));
        assert!(main.last_commit_at.is_some());
        let linked_entry = &entries[1];
        assert!(!linked_entry.is_main);
        assert_eq!(linked_entry.path, fs::canonicalize(&linked).unwrap());
        assert_eq!(linked_entry.branch.as_deref(), Some("occupied"));
        assert_eq!(linked_entry.dirty_files, Some(1));
        assert_eq!(linked_entry.ahead, None, "no upstream is configured");
        assert_eq!(linked_entry.head, main.head);

        // The same list answers from inside a linked worktree.
        assert_eq!(list_worktrees(&linked).unwrap().unwrap().len(), 2);

        // Outside a repository the read reports `None`.
        let outside = non_repository();
        assert!(list_worktrees(&outside).unwrap().is_none());
        fs::remove_dir_all(&outside).ok();

        fs::remove_dir_all(repository.parent().unwrap()).ok();
    }

    #[test]
    fn lists_local_and_remote_tracking_branches() {
        let repository = repository();
        // `--set-upstream-to` only accepts `origin/*` once `origin` is a real
        // remote, so the fixture pushes to a bare sibling and fetches back.
        let remote = repository.parent().unwrap().join("remote.git");
        run_git(
            repository.parent().unwrap(),
            &["init", "--bare", remote.to_str().unwrap()],
        );
        run_git(
            &repository,
            &["remote", "add", "origin", remote.to_str().unwrap()],
        );
        run_git(&repository, &["push", "origin", "main"]);
        run_git(&repository, &["push", "origin", "feature:refs/heads/topic"]);
        run_git(&repository, &["fetch", "origin"]);
        run_git(
            &repository,
            &[
                "symbolic-ref",
                "refs/remotes/origin/HEAD",
                "refs/remotes/origin/main",
            ],
        );
        run_git(
            &repository,
            &["branch", "--set-upstream-to=origin/main", "feature"],
        );
        // One commit ahead of the configured upstream.
        run_git(&repository, &["switch", "feature"]);
        fs::write(repository.join("FEATURE.md"), "feature\n").unwrap();
        commit(&repository, "feature work");
        run_git(&repository, &["switch", "main"]);

        let entries = list_repo_branches(&repository).unwrap().unwrap();
        let local = |name: &str| {
            entries
                .iter()
                .find(|branch| branch.remote.is_none() && branch.name == name)
                .unwrap_or_else(|| panic!("missing local branch {name}"))
        };
        let main = local("main");
        assert_eq!(main.last_commit_subject.as_deref(), Some("initial"));
        assert_eq!(main.checked_out_in.as_deref(), Some(repository.as_path()));
        assert_eq!(main.upstream, None);
        let feature = local("feature");
        assert_eq!(feature.upstream.as_deref(), Some("origin/main"));
        assert_eq!((feature.ahead, feature.behind), (Some(1), Some(0)));

        // Remote-tracking refs group under their remote; `origin/HEAD` is a
        // symref, not a branch, and stays out.
        let remote_names = entries
            .iter()
            .filter(|branch| branch.remote.as_deref() == Some("origin"))
            .map(|branch| branch.name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(remote_names, ["main", "topic"]);

        // Outside a repository the read reports `None`.
        let outside = non_repository();
        assert!(list_repo_branches(&outside).unwrap().is_none());
        fs::remove_dir_all(&outside).ok();

        fs::remove_dir_all(repository.parent().unwrap()).ok();
    }

    #[test]
    fn deletes_branches_and_collects_refusals() {
        let repository = repository();
        let linked = repository.with_extension("occupied");
        run_git(
            &repository,
            &[
                "worktree",
                "add",
                "-b",
                "occupied",
                linked.to_str().unwrap(),
            ],
        );

        // `feature` is merged — it still points at the initial commit —
        // while `occupied` is checked out in the linked worktree and Git
        // refuses to remove it.
        let failures = delete_branches(
            &repository,
            &["feature".to_owned(), "occupied".to_owned()],
            false,
        )
        .unwrap();
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].name, "occupied");
        assert!(!failures[0].error.is_empty());
        let remaining = list_repo_branches(&repository).unwrap().unwrap();
        assert!(
            !remaining
                .iter()
                .any(|branch| branch.remote.is_none() && branch.name == "feature")
        );
        assert!(
            remaining
                .iter()
                .any(|branch| branch.remote.is_none() && branch.name == "occupied")
        );

        // An unmerged branch refuses `-d` but yields to force.
        run_git(&repository, &["switch", "-c", "unmerged"]);
        fs::write(repository.join("UNMERGED.md"), "unmerged\n").unwrap();
        commit(&repository, "unmerged");
        run_git(&repository, &["switch", "main"]);
        let failures = delete_branches(&repository, &["unmerged".to_owned()], false).unwrap();
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].name, "unmerged");
        let failures = delete_branches(&repository, &["unmerged".to_owned()], true).unwrap();
        assert!(failures.is_empty());

        fs::remove_dir_all(repository.parent().unwrap()).ok();
    }
}
