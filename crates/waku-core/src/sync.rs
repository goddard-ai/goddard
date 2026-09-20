//! Friend-sync git engine — the blocking operations behind project
//! sharing. Everything here runs on the share worker thread; the tokio
//! runtime enqueues jobs and applies the published outcomes.
//!
//! A sync integrates `origin/<branch>` into a synced branch:
//! fast-forward when we're behind, rebase when diverged. The rebase runs
//! in the checkout that owns the branch — the main worktree, a linked
//! worktree, or a temp worktree we create for a branch checked out
//! nowhere. Conflicts stop the integration in place and surface as a
//! persisted [`SyncAlert`]; dirty checkouts are refused rather than
//! touched.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::bail;

use waku_protocol::git::SyncInProgress;
use waku_share::projects::{Integration, SyncAlert, SyncAlertKind, SyncLink};

use crate::git_commit::{
    command_error, ensure_repository, git_capture, git_optional_stdout, git_stdout, git_success,
};
use crate::git_panel::{conflicted_paths, sync_in_progress};

/// How one branch's integrate attempt ended.
#[derive(Debug)]
pub(crate) enum IntegrateOutcome {
    /// Local and remote tips agree, or the remote has no such branch.
    UpToDate,
    /// We were behind and fast-forwarded — includes tracking a branch
    /// that existed only on the remote.
    FastForwarded,
    /// Diverged and the rebase/merge completed.
    Integrated,
    /// We have commits the remote lacks — a push concern, not a pull.
    AheadOnly,
    /// The owning checkout has uncommitted changes — refused per spec.
    RefusedDirty { worktree: PathBuf },
    /// A rebase/merge is already stopped in the owning checkout — the
    /// existing alert (or the user's own work) has the floor.
    Busy,
    /// The integration stopped on conflicts; the alert owns the state.
    Conflict {
        in_progress: Integration,
        files: Vec<String>,
        worktree: PathBuf,
        temp_worktree: bool,
    },
}

fn rev_parse(repo: &Path, reference: &str) -> anyhow::Result<Option<String>> {
    git_optional_stdout(repo, &["rev-parse", "--verify", "--quiet", reference])
        .map(|tip| tip.filter(|tip| !tip.is_empty()))
}

fn current_branch(repo: &Path) -> anyhow::Result<Option<String>> {
    git_optional_stdout(repo, &["branch", "--show-current"]).map(|b| b.filter(|b| !b.is_empty()))
}

fn is_ancestor(repo: &Path, ancestor: &str, descendant: &str) -> anyhow::Result<bool> {
    Ok(git_capture(repo, &["merge-base", "--is-ancestor", ancestor, descendant])?
        .status
        .success())
}

/// Commits on `head` that `base` lacks.
fn ahead_count(repo: &Path, base: &str, head: &str) -> anyhow::Result<u64> {
    Ok(git_stdout(repo, &["rev-list", "--count", &format!("{base}..{head}")])?
        .parse()
        .unwrap_or(0))
}

/// Whether the checkout at `worktree` has staged, unstaged, or untracked
/// changes.
fn is_dirty(worktree: &Path) -> anyhow::Result<bool> {
    Ok(!git_stdout(worktree, &["status", "--porcelain=v1", "-z"])?.is_empty())
}

/// The worktree path that owns `branch`, if it's checked out anywhere —
/// `%(worktreepath)` is empty for available branches.
fn branch_worktree(repo: &Path, branch: &str) -> anyhow::Result<Option<PathBuf>> {
    let out = git_stdout(
        repo,
        &[
            "for-each-ref",
            "--format=%(refname:short)%00%(worktreepath)",
            &format!("refs/heads/{branch}"),
        ],
    )?;
    Ok(out
        .lines()
        .filter_map(|line| line.split_once('\0'))
        .find(|(name, _)| *name == branch)
        .and_then(|(_, path)| (!path.is_empty()).then(|| PathBuf::from(path))))
}

/// All local branch names, for the link config UI.
pub(crate) fn local_branches(repo: &Path) -> anyhow::Result<Vec<String>> {
    let out = git_stdout(repo, &["for-each-ref", "--format=%(refname:short)", "refs/heads"])?;
    Ok(out.lines().filter(|b| !b.is_empty()).map(str::to_owned).collect())
}

/// The repo's default branch: `origin/HEAD`'s target, else the checked-
/// out branch, else `main`/`master` when present.
pub(crate) fn default_branch(repo: &Path) -> anyhow::Result<Option<String>> {
    if let Some(remote_default) = git_optional_stdout(
        repo,
        &[
            "symbolic-ref",
            "--quiet",
            "--short",
            "refs/remotes/origin/HEAD",
        ],
    )? {
        if let Some(branch) = remote_default.strip_prefix("origin/") {
            return Ok(Some(branch.to_owned()));
        }
    }
    if let Some(current) = current_branch(repo)? {
        return Ok(Some(current));
    }
    for candidate in ["main", "master"] {
        if rev_parse(repo, &format!("refs/heads/{candidate}"))?.is_some() {
            return Ok(Some(candidate.to_owned()));
        }
    }
    Ok(None)
}

/// `git fetch origin` — refresh remote-tracking refs for a sync pass.
pub(crate) fn fetch(repo: &Path) -> anyhow::Result<()> {
    ensure_repository(repo)?;
    git_success(repo, &["fetch", "origin"])?;
    Ok(())
}

fn sanitize_component(raw: &str) -> String {
    let cleaned: String = raw
        .chars()
        .map(|c| if c == '/' || c.is_control() { '-' } else { c })
        .collect();
    let cleaned = cleaned.trim().trim_start_matches('.').trim_end();
    if cleaned.is_empty() {
        "sync".to_string()
    } else {
        cleaned.chars().take(60).collect()
    }
}

/// Remove a temp worktree we own — `git worktree remove --force` so a
/// stopped integration can't keep it registered.
pub(crate) fn remove_worktree(repo: &Path, path: &Path) -> anyhow::Result<()> {
    let output = git_capture(
        repo,
        &[
            "worktree",
            "remove",
            "--force",
            &path.to_string_lossy(),
        ],
    )?;
    if !output.status.success() {
        // Already gone or pruned is fine — anything else is noise-level.
        if path.exists() {
            bail!("{}", command_error(&output));
        }
    }
    Ok(())
}

/// Integrate `origin/<branch>` into `branch` of `repo`. `scratch_root`
/// roots temp worktrees for branches checked out nowhere.
pub(crate) fn integrate(
    repo: &Path,
    scratch_root: &Path,
    link_id: &str,
    branch: &str,
) -> anyhow::Result<IntegrateOutcome> {
    ensure_repository(repo)?;
    let local_ref = format!("refs/heads/{branch}");
    let remote_ref = format!("refs/remotes/origin/{branch}");
    let Some(remote_tip) = rev_parse(repo, &remote_ref)? else {
        return Ok(IntegrateOutcome::UpToDate);
    };
    let Some(local_tip) = rev_parse(repo, &local_ref)? else {
        // A synced branch that only exists on the remote — track it.
        git_success(repo, &["branch", branch, &remote_tip])?;
        return Ok(IntegrateOutcome::FastForwarded);
    };
    if local_tip == remote_tip || is_ancestor(repo, &remote_tip, &local_tip)? {
        // Synced, or we're strictly ahead — nothing to pull.
        return Ok(if local_tip == remote_tip {
            IntegrateOutcome::UpToDate
        } else {
            IntegrateOutcome::AheadOnly
        });
    }

    // Find the checkout that owns this branch: the main worktree if it's
    // current, a linked worktree otherwise, or a temp we create.
    let owned = current_branch(repo)?.as_deref() == Some(branch);
    let owner = if owned {
        Some(repo.to_path_buf())
    } else {
        branch_worktree(repo, branch)?
    };

    if is_ancestor(repo, &local_tip, &remote_tip)? {
        // Fast-forward. `branch -f` refuses a checked-out branch, so the
        // owning checkout merges ff-only itself — and must be clean.
        if let Some(worktree) = owner {
            if is_dirty(&worktree)? {
                return Ok(IntegrateOutcome::RefusedDirty { worktree });
            }
            let output = git_capture(&worktree, &["merge", "--ff-only", &remote_ref])?;
            if !output.status.success() {
                if is_dirty(&worktree)? {
                    return Ok(IntegrateOutcome::RefusedDirty { worktree });
                }
                bail!("{}", command_error(&output));
            }
        } else {
            git_success(repo, &["branch", "-f", branch, &remote_tip])?;
        }
        return Ok(IntegrateOutcome::FastForwarded);
    }

    // Diverged — rebase onto the remote tip.
    let (worktree, temp_worktree) = match owner {
        Some(worktree) => {
            if is_dirty(&worktree)? {
                return Ok(IntegrateOutcome::RefusedDirty { worktree });
            }
            if sync_in_progress(&worktree)?.is_some() {
                return Ok(IntegrateOutcome::Busy);
            }
            (worktree, false)
        }
        None => {
            let dir = scratch_root
                .join(sanitize_component(link_id))
                .join(sanitize_component(branch));
            if dir.exists() {
                // Stale from a crashed run — drop the registration and
                // the directory before re-adding.
                let _ = remove_worktree(repo, &dir);
                let _ = std::fs::remove_dir_all(&dir);
            }
            if let Some(parent) = dir.parent() {
                std::fs::create_dir_all(parent)?;
            }
            git_success(
                repo,
                &["worktree", "add", &dir.to_string_lossy(), branch],
            )?;
            (dir, true)
        }
    };

    let output = git_capture(&worktree, &["rebase", &remote_ref])?;
    if output.status.success() {
        if temp_worktree {
            remove_worktree(repo, &worktree)?;
        }
        return Ok(IntegrateOutcome::Integrated);
    }
    if let Some(in_progress) = sync_in_progress(&worktree)? {
        return Ok(IntegrateOutcome::Conflict {
            in_progress: match in_progress {
                SyncInProgress::Rebase => Integration::Rebase,
                SyncInProgress::Merge => Integration::Merge,
            },
            files: conflicted_paths(&worktree)?,
            worktree,
            temp_worktree,
        });
    }
    if temp_worktree {
        let _ = remove_worktree(repo, &worktree);
    }
    bail!("{}", command_error(&output));
}

/// Abort a stopped integration in `worktree`, then drop the temp
/// worktree registration when `temp_worktree`.
pub(crate) fn abort(
    repo: &Path,
    worktree: &Path,
    temp_worktree: bool,
) -> anyhow::Result<()> {
    if worktree.exists() {
        match sync_in_progress(worktree)? {
            Some(SyncInProgress::Rebase) => {
                git_success(worktree, &["rebase", "--abort"])?;
            }
            Some(SyncInProgress::Merge) => {
                git_success(worktree, &["merge", "--abort"])?;
            }
            None => {}
        }
    }
    if temp_worktree {
        let _ = remove_worktree(repo, worktree);
        if worktree.exists() {
            let _ = std::fs::remove_dir_all(worktree);
        }
    }
    Ok(())
}

/// "Merge instead": abort the stopped rebase in `worktree` and merge
/// `origin/<branch>` instead. Returns the new outcome — still conflicted
/// (now a merge) or integrated.
pub(crate) fn merge_instead(
    repo: &Path,
    worktree: &Path,
    branch: &str,
    temp_worktree: bool,
) -> anyhow::Result<IntegrateOutcome> {
    match sync_in_progress(worktree)? {
        Some(SyncInProgress::Rebase) => {
            git_success(worktree, &["rebase", "--abort"])?;
        }
        // A previous merge-instead that itself conflicted — start over.
        Some(SyncInProgress::Merge) => {
            git_success(worktree, &["merge", "--abort"])?;
        }
        None => {}
    }
    let remote_ref = format!("refs/remotes/origin/{branch}");
    let output = git_capture(worktree, &["merge", "--no-edit", &remote_ref])?;
    if output.status.success() {
        if temp_worktree {
            remove_worktree(repo, worktree)?;
        }
        return Ok(IntegrateOutcome::Integrated);
    }
    if let Some(in_progress) = sync_in_progress(worktree)? {
        return Ok(IntegrateOutcome::Conflict {
            in_progress: match in_progress {
                SyncInProgress::Rebase => Integration::Rebase,
                SyncInProgress::Merge => Integration::Merge,
            },
            files: conflicted_paths(worktree)?,
            worktree: worktree.to_path_buf(),
            temp_worktree,
        });
    }
    bail!("{}", command_error(&output));
}

/// One branch's remembered tips between poll cycles — the pending flag
/// is what distinguishes "our push completed" (notify) from "someone
/// else's fetch moved the ref" (stay quiet).
#[derive(Debug, Default)]
pub(crate) struct BranchTips {
    local: Option<String>,
    remote: Option<String>,
    pending_push: bool,
}

/// What a poll pass wants the runtime to do: push notices to send the
/// link's peer.
#[derive(Debug, Default)]
pub(crate) struct PollReport {
    /// Branches we auto-pushed this cycle.
    pub pushed: Vec<String>,
    /// Branches whose earlier local commits reached the remote by a push
    /// we didn't perform — still our commits, still worth a notice.
    pub noticed: Vec<String>,
}

/// Detect commits landing on synced branches (auto-push) and manual
/// pushes completing (notify). Pure local ref reads plus the push — no
/// fetch; freshness comes from notices and the periodic fetch job.
pub(crate) fn poll(
    repo: &Path,
    link: &SyncLink,
    states: &mut HashMap<String, BranchTips>,
) -> anyhow::Result<PollReport> {
    let mut report = PollReport::default();
    for branch in &link.enabled_branches {
        let local_ref = format!("refs/heads/{branch}");
        let remote_ref = format!("refs/remotes/origin/{branch}");
        let local = rev_parse(repo, &local_ref)?;
        let remote = rev_parse(repo, &remote_ref)?;
        let tips = states.entry(branch.clone()).or_default();

        if local != tips.local {
            let unpushed = match (&local, &remote) {
                (Some(local), Some(remote)) if local != remote => {
                    ahead_count(repo, remote, local)?
                }
                (Some(local), None) => git_stdout(
                    repo,
                    &["rev-list", "--count", local, "--not", "--remotes=origin"],
                )?
                .parse()
                .unwrap_or(0),
                _ => 0,
            };
            if unpushed > 0 {
                tips.pending_push = true;
            }
        }

        if tips.pending_push && link.auto_push && !link.paused_branches.contains(branch) {
            if let Some(local) = &local {
                let output = git_capture(
                    repo,
                    &[
                        "push",
                        "origin",
                        &format!("refs/heads/{branch}:refs/heads/{branch}"),
                    ],
                )?;
                if output.status.success() {
                    report.pushed.push(branch.clone());
                    tips.pending_push = false;
                    tips.local = Some(local.clone());
                    // The remote-tracking ref updates on our own push, so
                    // record it now and skip the `noticed` path below.
                    tips.remote = rev_parse(repo, &remote_ref)?;
                    continue;
                }
                // A rejected push means the remote moved — the sync side
                // integrates it and the next poll retries.
            }
        }

        if remote != tips.remote
            && tips.remote.is_some() // first sight isn't a move
            && let (Some(local), Some(remote_tip)) = (&local, &remote)
            && (local == remote_tip || is_ancestor(repo, local, remote_tip)?)
        {
            // The remote caught up to our head — a push landed our
            // commits. Usually we tracked it via pending_push; when the
            // push happened entirely between polls the exact-tip match
            // still fires so the friend hears about it.
            if tips.pending_push || local == remote_tip {
                report.noticed.push(branch.clone());
            }
            tips.pending_push = false;
        }
        tips.local = local;
        tips.remote = remote;
    }
    Ok(report)
}

/// Reconcile an alert against the on-disk truth: a conflict whose
/// integration is gone was resolved or aborted outside us. `None` means
/// the alert still stands (not a conflict, or still stopped). `Some(true)`
/// means it ended in an external abort — pause the branch exactly as if
/// we had aborted it. `Some(false)` means it resolved — drop the alert.
pub(crate) fn reconcile_alert(repo: &Path, alert: &SyncAlert) -> anyhow::Result<Option<bool>> {
    if alert.kind != SyncAlertKind::Conflict {
        return Ok(None);
    }
    let worktree = &alert.worktree_path;
    if !worktree.exists() {
        return Ok(Some(true));
    }
    if sync_in_progress(worktree)?.is_some() {
        return Ok(None); // still stopped — the alert stands
    }
    let remote_ref = format!("refs/remotes/origin/{}", alert.branch);
    let remote = rev_parse(repo, &remote_ref)?;
    let local = rev_parse(repo, &format!("refs/heads/{}", alert.branch))?;
    let resolved = match (&local, &remote) {
        (Some(local), Some(remote)) => {
            local == remote || is_ancestor(repo, remote, local)?
        }
        _ => false,
    };
    Ok(Some(!resolved))
}

/// Create an alert record for an integrate outcome. Caller inserts into
/// the store and publishes.
pub(crate) fn conflict_alert(
    link: &SyncLink,
    branch: &str,
    in_progress: Integration,
    files: Vec<String>,
    worktree: PathBuf,
    temp_worktree: bool,
) -> SyncAlert {
    SyncAlert {
        id: uuid::Uuid::new_v4().to_string(),
        link_id: link.id.clone(),
        branch: branch.to_string(),
        kind: SyncAlertKind::Conflict,
        in_progress: Some(in_progress),
        files,
        worktree_path: worktree,
        temp_worktree,
        at_ms: now_ms(),
    }
}

pub(crate) fn refused_alert(link: &SyncLink, branch: &str, worktree: PathBuf) -> SyncAlert {
    SyncAlert {
        id: uuid::Uuid::new_v4().to_string(),
        link_id: link.id.clone(),
        branch: branch.to_string(),
        kind: SyncAlertKind::RefusedDirtyWorktree,
        in_progress: None,
        files: Vec::new(),
        worktree_path: worktree,
        temp_worktree: false,
        at_ms: now_ms(),
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeSet, HashMap};
    use std::fs;
    use uuid::Uuid;

    fn run_git(cwd: &Path, args: &[&str]) {
        let output = crate::command_env::plain_command("git")
            .args(args)
            .current_dir(cwd)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn commit(cwd: &Path, name: &str, content: &str) {
        fs::write(cwd.join(name), content).unwrap();
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
                name,
            ],
        );
    }

    /// A bare "origin" plus two clones — `friend` pushes, `ours` syncs.
    fn fixture() -> (PathBuf, PathBuf, PathBuf) {
        let root = std::env::temp_dir().join(format!("waku-sync-test-{}", Uuid::new_v4()));
        let seed = root.join("seed");
        let remote = root.join("remote.git");
        let friend = root.join("friend");
        let ours = root.join("ours");
        fs::create_dir_all(&seed).unwrap();
        run_git(&seed, &["init", "-b", "main"]);
        commit(&seed, "README.md", "initial\n");
        run_git(&seed, &["clone", "--bare", ".", remote.to_str().unwrap()]);
        run_git(&seed, &["clone", remote.to_str().unwrap(), friend.to_str().unwrap()]);
        run_git(&seed, &["clone", remote.to_str().unwrap(), ours.to_str().unwrap()]);
        // integrate()'s rebase and merge paths commit without -c flags, so
        // the repos that sync need a configured identity on hosts (CI) that
        // have no global one.
        for repo in [&friend, &ours] {
            run_git(repo, &["config", "user.name", "Goddard Tests"]);
            run_git(repo, &["config", "user.email", "waku@example.com"]);
        }
        (remote, friend, ours)
    }

    fn link(repo_path: &Path, branches: &[&str], auto_push: bool) -> SyncLink {
        SyncLink {
            id: "test-link".into(),
            // The ed25519 identity point — a real key isn't needed, only a
            // structurally valid one.
            peer: waku_share::EndpointId::from_bytes(&{
                let mut bytes = [0u8; 32];
                bytes[0] = 1;
                bytes
            })
            .unwrap(),
            origin_url: "origin".into(),
            repo_path: repo_path.to_path_buf(),
            auto_push,
            enabled_branches: branches.iter().map(|b| b.to_string()).collect::<BTreeSet<_>>(),
            paused_branches: BTreeSet::new(),
            peer_sync_enabled: true,
            created_at_ms: 0,
        }
    }

    fn tip(repo: &Path, reference: &str) -> String {
        rev_parse(repo, reference).unwrap().unwrap()
    }

    #[test]
    fn fast_forwards_a_behind_branch() {
        let (_remote, friend, ours) = fixture();
        commit(&friend, "UP.md", "friend\n");
        run_git(&friend, &["push", "origin", "main"]);
        fetch(&ours).unwrap();

        let scratch = ours.join("scratch");
        let outcome = integrate(&ours, &scratch, "link", "main").unwrap();
        assert!(matches!(outcome, IntegrateOutcome::FastForwarded));
        assert_eq!(tip(&ours, "refs/heads/main"), tip(&ours, "refs/remotes/origin/main"));
    }

    #[test]
    fn rebases_a_diverged_checkout() {
        let (_remote, friend, ours) = fixture();
        commit(&friend, "FRIEND.md", "theirs\n");
        run_git(&friend, &["push", "origin", "main"]);
        commit(&ours, "OURS.md", "ours\n");
        fetch(&ours).unwrap();

        let scratch = ours.join("scratch");
        let outcome = integrate(&ours, &scratch, "link", "main").unwrap();
        assert!(matches!(outcome, IntegrateOutcome::Integrated));
        assert!(ours.join("OURS.md").exists());
        assert!(ours.join("FRIEND.md").exists());
        // Our commit replays on top — the remote tip is now an ancestor.
        assert!(is_ancestor(&ours, "refs/remotes/origin/main", "refs/heads/main").unwrap());
    }

    #[test]
    fn refuses_a_dirty_checked_out_branch() {
        let (_remote, friend, ours) = fixture();
        commit(&friend, "FRIEND.md", "theirs\n");
        run_git(&friend, &["push", "origin", "main"]);
        commit(&ours, "OURS.md", "ours\n");
        fs::write(ours.join("README.md"), "dirty\n").unwrap();
        fetch(&ours).unwrap();

        let scratch = ours.join("scratch");
        let outcome = integrate(&ours, &scratch, "link", "main").unwrap();
        assert!(matches!(outcome, IntegrateOutcome::RefusedDirty { .. }));
        // The rebase never started — local tip unchanged.
        assert!(!is_ancestor(&ours, "refs/remotes/origin/main", "refs/heads/main").unwrap());
    }

    #[test]
    fn conflict_alerts_then_abort_and_merge() {
        let (_remote, friend, ours) = fixture();
        commit(&friend, "README.md", "friend wins\n");
        run_git(&friend, &["push", "origin", "main"]);
        commit(&ours, "README.md", "ours wins\n");
        fetch(&ours).unwrap();

        let scratch = ours.join("scratch");
        let outcome = integrate(&ours, &scratch, "link", "main").unwrap();
        let IntegrateOutcome::Conflict {
            in_progress,
            files,
            worktree,
            temp_worktree,
        } = outcome
        else {
            panic!("expected a conflict, got {outcome:?}");
        };
        assert_eq!(in_progress, Integration::Rebase);
        assert_eq!(files, vec!["README.md".to_string()]);
        assert!(!temp_worktree);

        // Abort restores the pre-sync tip and merge-instead integrates.
        abort(&ours, &worktree, false).unwrap();
        assert!(sync_in_progress(&ours).unwrap().is_none());
        let outcome = merge_instead(&ours, &ours, "main", false).unwrap();
        let IntegrateOutcome::Conflict { in_progress, .. } = outcome else {
            panic!("merge should conflict too, got {outcome:?}");
        };
        assert_eq!(in_progress, Integration::Merge);
        abort(&ours, &ours, false).unwrap();
        assert!(sync_in_progress(&ours).unwrap().is_none());
    }

    #[test]
    fn diverged_branch_nowhere_checked_out_uses_a_temp_worktree() {
        let (_remote, friend, ours) = fixture();
        // Local `topic` diverges from the remote copy the friend pushed —
        // `main` stays checked out, so sync must build a temp worktree.
        run_git(&ours, &["checkout", "-b", "topic"]);
        commit(&ours, "OURS.md", "ours\n");
        run_git(&ours, &["checkout", "main"]);
        commit(&friend, "FRIEND.md", "theirs\n");
        run_git(&friend, &["push", "origin", "main:topic"]);
        fetch(&ours).unwrap();

        let scratch = ours.join("scratch");
        let outcome = integrate(&ours, &scratch, "link", "topic").unwrap();
        assert!(matches!(outcome, IntegrateOutcome::Integrated));
        // The temp worktree came and went; the branch tip moved.
        assert!(is_ancestor(&ours, "refs/remotes/origin/topic", "refs/heads/topic").unwrap());
        assert!(!scratch.join("link/topic").exists());
    }

    #[test]
    fn poll_pushes_new_commits_and_notices_once() {
        let (_remote, _friend, ours) = fixture();
        let link = link(&ours, &["main"], true);
        let mut states = HashMap::new();

        // First poll learns the tips — nothing to report.
        let report = poll(&ours, &link, &mut states).unwrap();
        assert!(report.pushed.is_empty() && report.noticed.is_empty());

        commit(&ours, "OURS.md", "new commit\n");
        let report = poll(&ours, &link, &mut states).unwrap();
        assert_eq!(report.pushed, vec!["main".to_string()]);
        assert_eq!(tip(&ours, "refs/remotes/origin/main"), tip(&ours, "refs/heads/main"));

        // A steady remote reports nothing further.
        let report = poll(&ours, &link, &mut states).unwrap();
        assert!(report.pushed.is_empty() && report.noticed.is_empty());
    }

    #[test]
    fn paused_branches_do_not_auto_push() {
        let (_remote, _friend, ours) = fixture();
        let mut link = link(&ours, &["main"], true);
        let mut states = HashMap::new();
        poll(&ours, &link, &mut states).unwrap();

        link.paused_branches.insert("main".into());
        commit(&ours, "OURS.md", "held back\n");
        let report = poll(&ours, &link, &mut states).unwrap();
        assert!(report.pushed.is_empty());
        assert_ne!(tip(&ours, "refs/remotes/origin/main"), tip(&ours, "refs/heads/main"));
    }

    #[test]
    fn auto_push_off_notices_manual_pushes() {
        let (_remote, _friend, ours) = fixture();
        let link = link(&ours, &["main"], false);
        let mut states = HashMap::new();
        poll(&ours, &link, &mut states).unwrap();

        commit(&ours, "OURS.md", "manual\n");
        let report = poll(&ours, &link, &mut states).unwrap();
        // auto_push off — nothing pushed yet.
        assert!(report.pushed.is_empty() && report.noticed.is_empty());

        run_git(&ours, &["push", "origin", "main"]);
        let report = poll(&ours, &link, &mut states).unwrap();
        assert_eq!(report.noticed, vec!["main".to_string()]);
    }

    #[test]
    fn external_push_between_polls_still_notices() {
        let (_remote, _friend, ours) = fixture();
        let link = link(&ours, &["main"], true);
        let mut states = HashMap::new();
        poll(&ours, &link, &mut states).unwrap();

        // Commit and push entirely between polls — no poll ever saw the
        // branch ahead, yet the remote tip equals our head now, so the
        // friend still deserves a notice.
        commit(&ours, "OURS.md", "out of band\n");
        run_git(&ours, &["push", "origin", "main"]);
        let report = poll(&ours, &link, &mut states).unwrap();
        assert_eq!(report.noticed, vec!["main".to_string()]);

        // Steady state: no repeat notices.
        let report = poll(&ours, &link, &mut states).unwrap();
        assert!(report.noticed.is_empty());
    }

    #[test]
    fn remote_moving_without_our_push_does_not_notice() {
        let (_remote, friend, ours) = fixture();
        let link = link(&ours, &["main"], true);
        let mut states = HashMap::new();
        poll(&ours, &link, &mut states).unwrap();

        // The friend pushed commits we don't have — remote moved but our
        // head isn't its tip, so nothing to tell them about.
        commit(&friend, "FRIEND.md", "theirs\n");
        run_git(&friend, &["push", "origin", "main"]);
        fetch(&ours).unwrap();
        let report = poll(&ours, &link, &mut states).unwrap();
        assert!(report.pushed.is_empty() && report.noticed.is_empty());
    }
}
