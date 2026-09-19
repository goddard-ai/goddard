//! QA-branch review state for the Projects page's Review tab.
//!
//! Proposed work lands on `origin/qa` unreviewed; human decisions are git
//! notes under `refs/notes/qa` — one JSON record per line so concurrent
//! reviewers union-merge cleanly. The base branch fast-forwards to the
//! longest approved prefix. Whether a commit needs a human is policy
//! computed locally from `Test-Plan:` trailers and sensitive paths; only
//! decisions are synced state. Rejection reverts the commit on `qa`
//! rather than blocking the train.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use anyhow::{Context as _, bail};
use serde::{Deserialize, Serialize};

use waku_protocol::git::{ReviewDecision, ReviewEntry, ReviewQueue, ReviewRecord};

use crate::git_commit::{
    command_error, ensure_repository, git_capture, git_optional_stdout, git_stdout, git_success,
};

/// The shared proposed-work branch, read through its remote-tracking ref.
const QA_REMOTE: &str = "refs/remotes/origin/qa";
/// The notes ref review records live under — pushed to `origin` on every
/// decision. `pub(crate)` so the daemon can name it in friend notices.
pub(crate) const NOTES_REF: &str = "refs/notes/qa";
/// Scratch ref a losing notes push fetches into before union-merging.
const NOTES_REMOTE_TMP: &str = "refs/notes/qa-remote";
/// How far back the queue reads `origin/qa`.
const QUEUE_LIMIT: usize = 200;
/// Notes pushes retry this many fetch+merge cycles before giving up.
const PUSH_ATTEMPTS: usize = 3;
/// Revert pushes retry when `qa` moved under the attempt.
const REVERT_ATTEMPTS: usize = 2;

/// One stored record — a line in a commit's note. Unknown lines are
/// ignored, so the format can grow fields without breaking old readers.
#[derive(Debug, Deserialize, Serialize)]
struct NoteLine {
    v: u8,
    reviewer: String,
    decision: ReviewDecision,
    /// Unix seconds.
    at: u64,
}

/// The review queue for `origin/qa`, plus the promotable frontier.
/// Fetches `origin` and the notes ref first so the view is fresh; fetch
/// failures degrade to last-known state rather than an empty queue.
/// `None` outside a repository.
pub fn queue(cwd: &Path) -> anyhow::Result<Option<ReviewQueue>> {
    if git_optional_stdout(cwd, &["rev-parse", "--git-dir"])?.is_none() {
        return Ok(None);
    }
    let _ = git_capture(cwd, &["fetch", "origin"]);
    fetch_notes(cwd);
    queue_inner(cwd).map(Some)
}

/// Approve `sha`: note it and return the refreshed queue.
pub fn approve(cwd: &Path, sha: &str) -> anyhow::Result<Option<ReviewQueue>> {
    record(cwd, sha, ReviewDecision::Approved)?;
    queue_inner(cwd).map(Some)
}

/// Reject `sha`: note it, revert it on `qa`, push both, and return the
/// refreshed queue. The revert runs in a temp worktree at `origin/qa`'s
/// tip — the reviewer's own checkouts are never touched.
pub fn reject(cwd: &Path, sha: &str) -> anyhow::Result<Option<ReviewQueue>> {
    record(cwd, sha, ReviewDecision::Rejected)?;
    revert_on_qa(cwd, sha)?;
    queue_inner(cwd).map(Some)
}

/// Fast-forward the base branch to the approved frontier and return the
/// refreshed queue. A non-fast-forward push means the base moved without
/// `qa` (a hotfix) — git's error says as much.
pub fn promote(cwd: &Path) -> anyhow::Result<Option<ReviewQueue>> {
    let queue = queue(cwd)?.context("not inside a repository")?;
    let Some(frontier) = &queue.frontier else {
        bail!("nothing on qa is approved for promotion");
    };
    let base = queue.base_branch.clone().unwrap_or_else(|| "main".to_owned());
    git_success(
        cwd,
        &["push", "origin", &format!("{frontier}:refs/heads/{base}")],
    )?;
    queue_inner(cwd).map(Some)
}

/// Fetch moved refs into `cwd` after a friend's ref notice. Best-effort —
/// a stale view just waits for the next sync pass.
pub fn fetch_refs(cwd: &Path, refs: &[String]) {
    if refs.is_empty() || !cwd.join(".git").exists() {
        return;
    }
    let mut args = vec!["fetch".to_owned(), "origin".to_owned()];
    args.extend(refs.iter().map(|r| format!("+{r}:{r}")));
    let _ = git_capture(cwd, &args.iter().map(String::as_str).collect::<Vec<_>>());
}

fn queue_inner(cwd: &Path) -> anyhow::Result<ReviewQueue> {
    let base = crate::sync::default_branch(cwd)?.unwrap_or_else(|| "main".to_owned());
    let mut queue = ReviewQueue {
        base_branch: Some(base.clone()),
        ..Default::default()
    };
    if rev_parse(cwd, QA_REMOTE)?.is_none() {
        return Ok(queue);
    }
    let base_ref = format!("refs/remotes/origin/{base}");
    // No base ref yet → the whole branch is proposed work.
    let range = if rev_parse(cwd, &base_ref)?.is_some() {
        format!("{base_ref}..{QA_REMOTE}")
    } else {
        QA_REMOTE.to_owned()
    };
    let mut commits = crate::git_panel::log_commits(cwd, &[range.clone()], 0, QUEUE_LIMIT)?;
    let notes = notes_by_commit(cwd, &range)?;
    let paths = paths_by_commit(cwd, &range)?;
    // `git revert` records "This reverts commit <sha>." — a reverted
    // commit's changes are gone, so it can't block the frontier even
    // when its review is still pending.
    let reverted: std::collections::HashSet<String> = commits
        .iter()
        .flat_map(|commit| revert_marks(&commit.body))
        .collect();
    commits.reverse(); // git log is newest-first; the queue is oldest-first.
    let mut promotable = true;
    for commit in commits {
        let test_plans = test_plans(&commit.body);
        let reviews = latest_reviews(notes.get(&commit.sha));
        let rejected = reviews
            .iter()
            .any(|r| r.decision == ReviewDecision::Rejected);
        let approved_by_review = reviews
            .iter()
            .any(|r| r.decision == ReviewDecision::Approved);
        let sensitive = paths
            .get(&commit.sha)
            .is_some_and(|paths| paths.iter().any(|p| is_sensitive_path(p)));
        let needs_review = !test_plans.is_empty() || sensitive;
        let approved = !rejected && (!needs_review || approved_by_review);
        let reverted = reverted.contains(&commit.sha);
        // The frontier is the longest resolved prefix — one unapproved,
        // unreverted commit ends it no matter what follows.
        if (approved || reverted) && promotable {
            queue.frontier = Some(commit.sha.clone());
        } else {
            promotable = false;
        }
        queue.entries.push(ReviewEntry {
            commit,
            test_plans,
            needs_review,
            reviews,
            rejected,
            reverted,
            approved,
        });
    }
    Ok(queue)
}

/// `sha` → stored review records, read in one `git log --notes` pass.
fn notes_by_commit(cwd: &Path, range: &str) -> anyhow::Result<HashMap<String, Vec<NoteLine>>> {
    let output = git_stdout(
        cwd,
        &["log", "--notes=qa", "--format=%H%x1f%N%x1e", range],
    )?;
    let mut map = HashMap::new();
    for record in output.split('\x1e') {
        let mut fields = record.splitn(2, '\x1f');
        let Some(sha) = fields.next().map(str::trim).filter(|s| !s.is_empty()) else {
            continue;
        };
        let lines: Vec<NoteLine> = fields
            .next()
            .unwrap_or_default()
            .lines()
            .filter(|line| !line.trim().is_empty())
            .filter_map(|line| serde_json::from_str(line.trim()).ok())
            .collect();
        if !lines.is_empty() {
            map.insert(sha.to_owned(), lines);
        }
    }
    Ok(map)
}

/// `sha` → touched paths, one `--name-only` pass for sensitive-path policy.
fn paths_by_commit(cwd: &Path, range: &str) -> anyhow::Result<HashMap<String, Vec<String>>> {
    let output = git_stdout(cwd, &["log", "--format=%x1e%H", "--name-only", range])?;
    let mut map = HashMap::new();
    for record in output.split('\x1e') {
        let mut lines = record.lines();
        let Some(sha) = lines.next().map(str::trim).filter(|s| !s.is_empty()) else {
            continue;
        };
        map.insert(
            sha.to_owned(),
            lines
                .map(str::trim)
                .filter(|p| !p.is_empty())
                .map(str::to_owned)
                .collect(),
        );
    }
    Ok(map)
}

/// Shas a commit's body claims to revert — `git revert` writes
/// "This reverts commit <sha>." with the full 40-char sha.
fn revert_marks(body: &str) -> Vec<String> {
    body.lines()
        .filter_map(|line| {
            let sha = line
                .trim()
                .strip_prefix("This reverts commit ")?
                .trim_end_matches('.');
            (sha.len() == 40 && sha.chars().all(|c| c.is_ascii_hexdigit()))
                .then(|| sha.to_owned())
        })
        .collect()
}

/// `Test-Plan:` trailer values from a commit body — each is one check a
/// reviewer runs.
fn test_plans(body: &str) -> Vec<String> {
    body.lines()
        .filter_map(|line| {
            let (key, value) = line.trim().split_once(':')?;
            key.trim()
                .eq_ignore_ascii_case("test-plan")
                .then(|| value.trim().to_owned())
                .filter(|v| !v.is_empty())
        })
        .collect()
}

/// Paths that always need a human, whatever the message says: CI
/// definitions, dependency manifests and lockfiles, secret-adjacent
/// files.
fn is_sensitive_path(path: &str) -> bool {
    let lower = path.to_lowercase();
    let name = lower.rsplit('/').next().unwrap_or(&lower);
    lower.starts_with(".github/")
        || matches!(
            name,
            "cargo.toml"
                | "cargo.lock"
                | "package.json"
                | "package-lock.json"
                | "bun.lock"
                | "bun.lockb"
                | "pnpm-lock.yaml"
                | "yarn.lock"
        )
        || name.starts_with(".env")
        || name.contains("secret")
        || name.contains("credential")
}

/// Latest decision per reviewer — a reviewer who re-decides replaces
/// their earlier record.
fn latest_reviews(lines: Option<&Vec<NoteLine>>) -> Vec<ReviewRecord> {
    let mut by_reviewer: BTreeMap<&str, &NoteLine> = BTreeMap::new();
    for line in lines.into_iter().flatten() {
        by_reviewer
            .entry(line.reviewer.as_str())
            .and_modify(|current| {
                if line.at >= current.at {
                    *current = line;
                }
            })
            .or_insert(line);
    }
    by_reviewer
        .values()
        .map(|line| ReviewRecord {
            reviewer: line.reviewer.clone(),
            decision: line.decision,
            at: line.at,
        })
        .collect()
}

/// `user.name <user.email>` — the identity a review record claims.
fn reviewer_identity(cwd: &Path) -> anyhow::Result<String> {
    let name = git_optional_stdout(cwd, &["config", "user.name"])?.unwrap_or_default();
    let email = git_optional_stdout(cwd, &["config", "user.email"])?.unwrap_or_default();
    match (name.is_empty(), email.is_empty()) {
        (false, false) => Ok(format!("{name} <{email}>")),
        (false, true) => Ok(name),
        (true, false) => Ok(email),
        (true, true) => bail!("set git user.name and user.email to record reviews"),
    }
}

/// Record `decision` for `sha` — it must still be on `origin/qa` — then
/// push the notes ref.
fn record(cwd: &Path, sha: &str, decision: ReviewDecision) -> anyhow::Result<()> {
    ensure_repository(cwd)?;
    fetch_notes(cwd);
    let on_qa = git_capture(cwd, &["merge-base", "--is-ancestor", sha, QA_REMOTE])?
        .status
        .success();
    if !on_qa {
        bail!("{sha} is not on origin/qa — it may have been promoted or reverted already");
    }
    let line = serde_json::to_string(&NoteLine {
        v: 1,
        reviewer: reviewer_identity(cwd)?,
        decision,
        at: now_secs(),
    })?;
    let existing = git_optional_stdout(cwd, &["notes", "--ref", "qa", "show", sha])?
        .unwrap_or_default();
    let content = match existing.trim_end() {
        "" => line,
        existing => format!("{existing}\n{line}"),
    };
    // Unique per call — a concurrent record (or test) must not read a
    // stranger's decision.
    let tmp =
        std::env::temp_dir().join(format!("goddard-qa-note-{}", uuid::Uuid::new_v4()));
    std::fs::write(&tmp, format!("{content}\n"))?;
    let write = git_success(
        cwd,
        &[
            "notes",
            "--ref",
            "qa",
            "add",
            "-f",
            "-F",
            &tmp.to_string_lossy(),
            sha,
        ],
    )
    .map(|_| ());
    let _ = std::fs::remove_file(&tmp);
    write?;
    push_notes(cwd)
}

/// Revert `sha` on top of `origin/qa` in a temp worktree and push
/// `HEAD:qa`. Retries when `qa` moved under the attempt.
fn revert_on_qa(cwd: &Path, sha: &str) -> anyhow::Result<()> {
    let dir = std::env::temp_dir().join(format!("goddard-qa-{}", uuid::Uuid::new_v4()));
    let mut last_error = None;
    for _ in 0..REVERT_ATTEMPTS {
        let _ = git_capture(cwd, &["fetch", "origin", "qa"]);
        if dir.exists() {
            let _ = crate::sync::remove_worktree(cwd, &dir);
            let _ = std::fs::remove_dir_all(&dir);
        }
        git_success(
            cwd,
            &[
                "worktree",
                "add",
                "--detach",
                &dir.to_string_lossy(),
                QA_REMOTE,
            ],
        )?;
        let attempt = revert_and_push(&dir, sha);
        let _ = crate::sync::remove_worktree(cwd, &dir);
        let _ = std::fs::remove_dir_all(&dir);
        match attempt {
            Ok(()) => return Ok(()),
            Err(error) => last_error = Some(error),
        }
    }
    Err(last_error.unwrap_or_else(|| anyhow::anyhow!("revert did not run")))
}

fn revert_and_push(worktree: &Path, sha: &str) -> anyhow::Result<()> {
    let output = git_capture(worktree, &["revert", "--no-edit", sha])?;
    if !output.status.success() {
        let files = crate::git_panel::conflicted_paths(worktree).unwrap_or_default();
        let _ = git_capture(worktree, &["revert", "--abort"]);
        let short = &sha[..sha.len().min(12)];
        bail!(
            "reverting {short} on qa conflicts{}",
            if files.is_empty() {
                format!(": {}", command_error(&output))
            } else {
                format!(" in {}", files.join(", "))
            }
        );
    }
    git_success(worktree, &["push", "origin", "HEAD:refs/heads/qa"])?;
    Ok(())
}

/// Push `refs/notes/qa`, union-merging the remote's copy on a lost race —
/// one record per line makes `cat_sort_uniq` the correct merge.
fn push_notes(cwd: &Path) -> anyhow::Result<()> {
    for _ in 0..PUSH_ATTEMPTS {
        let output = git_capture(cwd, &["push", "origin", &format!("{NOTES_REF}:{NOTES_REF}")])?;
        if output.status.success() {
            return Ok(());
        }
        git_success(
            cwd,
            &["fetch", "origin", &format!("{NOTES_REF}:{NOTES_REMOTE_TMP}")],
        )?;
        git_success(
            cwd,
            &[
                "notes",
                "--ref",
                "qa",
                "merge",
                "-s",
                "cat_sort_uniq",
                NOTES_REMOTE_TMP,
            ],
        )?;
        let _ = git_capture(cwd, &["update-ref", "-d", NOTES_REMOTE_TMP]);
    }
    bail!("pushing {NOTES_REF} kept losing the race")
}

fn fetch_notes(cwd: &Path) {
    let _ = git_capture(cwd, &["fetch", "origin", &format!("+{NOTES_REF}:{NOTES_REF}")]);
}

fn rev_parse(cwd: &Path, reference: &str) -> anyhow::Result<Option<String>> {
    git_optional_stdout(cwd, &["rev-parse", "--verify", "--quiet", reference])
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::process::Command;
    use uuid::Uuid;

    fn run_git(dir: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {args:?} failed in {}: {}",
            dir.display(),
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    }

    fn commit(dir: &Path, file: &str, contents: &str, message: &str) -> String {
        fs::write(dir.join(file), contents).unwrap();
        run_git(dir, &["add", file]);
        run_git(dir, &["commit", "-m", message]);
        run_git(dir, &["rev-parse", "HEAD"])
    }

    /// A bare `origin` plus one clone with `main` and `qa` pushed — `qa`
    /// starts at `main`, commits go on `qa` via `propose`.
    fn fixture() -> (PathBuf, PathBuf) {
        let root = std::env::temp_dir().join(format!("waku-review-test-{}", Uuid::new_v4()));
        let seed = root.join("seed");
        let remote = root.join("remote.git");
        let ours = root.join("ours");
        fs::create_dir_all(&seed).unwrap();
        run_git(&seed, &["init", "-b", "main"]);
        commit(&seed, "README.md", "initial\n", "initial");
        run_git(&seed, &["clone", "--bare", ".", remote.to_str().unwrap()]);
        run_git(
            &seed,
            &["clone", remote.to_str().unwrap(), ours.to_str().unwrap()],
        );
        run_git(&ours, &["config", "user.name", "Tester"]);
        run_git(&ours, &["config", "user.email", "tester@example.com"]);
        run_git(&ours, &["branch", "qa"]);
        run_git(&ours, &["push", "origin", "qa"]);
        (remote, ours)
    }

    /// Land one proposed commit on the `qa` branch and push it.
    fn propose(ours: &Path, file: &str, contents: &str, message: &str) -> String {
        run_git(ours, &["checkout", "qa"]);
        let sha = commit(ours, file, contents, message);
        run_git(ours, &["push", "origin", "qa"]);
        run_git(ours, &["checkout", "main"]);
        sha
    }

    #[test]
    fn queue_orders_oldest_first_and_frontier_stops_at_unreviewed() {
        let (_remote, ours) = fixture();
        let plain = propose(&ours, "plain.txt", "plain\n", "docs tweak");
        let _reviewed = propose(
            &ours,
            "feature.rs",
            "code\n",
            "add feature\n\nTest-Plan: run the thing; it prints ok",
        );

        let queue = queue(&ours).unwrap().unwrap();
        assert_eq!(queue.entries.len(), 2);
        assert_eq!(queue.entries[0].commit.sha, plain);
        assert!(!queue.entries[0].needs_review);
        assert!(queue.entries[0].approved);
        assert_eq!(
            queue.entries[1].test_plans,
            vec!["run the thing; it prints ok"]
        );
        assert!(queue.entries[1].needs_review);
        assert!(!queue.entries[1].approved);
        // The trailer'd commit ends the prefix — frontier is the first.
        assert_eq!(queue.frontier.as_deref(), Some(plain.as_str()));
    }

    #[test]
    fn sensitive_paths_need_review_without_a_trailer() {
        let (_remote, ours) = fixture();
        propose(&ours, "package.json", "{}\n", "bump dep");
        let queue = queue(&ours).unwrap().unwrap();
        assert!(queue.entries[0].needs_review);
        assert!(!queue.entries[0].approved);
        assert!(queue.frontier.is_none());
    }

    #[test]
    fn approve_records_a_note_and_promote_lands_the_frontier() {
        let (remote, ours) = fixture();
        propose(&ours, "plain.txt", "plain\n", "docs tweak");
        let reviewed = propose(
            &ours,
            "feature.rs",
            "code\n",
            "add feature\n\nTest-Plan: run the thing",
        );

        let queue = approve(&ours, &reviewed).unwrap().unwrap();
        assert_eq!(queue.frontier.as_deref(), Some(reviewed.as_str()));
        let entry = &queue.entries[1];
        assert!(entry.approved);
        assert_eq!(entry.reviews.len(), 1);
        assert_eq!(entry.reviews[0].reviewer, "Tester <tester@example.com>");
        assert_eq!(entry.reviews[0].decision, ReviewDecision::Approved);

        let queue = promote(&ours).unwrap().unwrap();
        assert!(queue.entries.is_empty());
        assert_eq!(
            run_git(&Path::new(&remote), &["rev-parse", "refs/heads/main"]),
            reviewed
        );
    }

    #[test]
    fn reject_records_and_reverts_so_the_train_moves() {
        let (_remote, ours) = fixture();
        let bad = propose(&ours, "bad.txt", "bad\n", "break things");
        propose(&ours, "good.txt", "good\n", "docs tweak");

        let queue = reject(&ours, &bad).unwrap().unwrap();
        // The bad commit gained a revert on qa; its tree change is gone.
        run_git(&ours, &["fetch", "origin", "qa"]);
        run_git(&ours, &["checkout", "qa"]);
        run_git(&ours, &["merge", "--ff-only", "origin/qa"]);
        assert!(!ours.join("bad.txt").exists());
        assert!(ours.join("good.txt").exists());

        // Queue = bad, good, revert — bad shows rejected and reverted,
        // the rest are auto-approved, so the frontier is the revert's
        // sha.
        assert_eq!(queue.entries.len(), 3);
        assert!(queue.entries[0].rejected);
        assert!(queue.entries[0].reverted);
        assert!(!queue.entries[0].approved);
        assert_eq!(
            queue.frontier.as_deref(),
            Some(queue.entries[2].commit.sha.as_str())
        );
    }

    #[test]
    fn latest_decision_per_reviewer_wins() {
        let approved = NoteLine {
            v: 1,
            reviewer: "a".into(),
            decision: ReviewDecision::Approved,
            at: 10,
        };
        let rejected_earlier = NoteLine {
            v: 1,
            reviewer: "a".into(),
            decision: ReviewDecision::Rejected,
            at: 5,
        };
        let rejected_newer = NoteLine {
            v: 1,
            reviewer: "b".into(),
            decision: ReviewDecision::Rejected,
            at: 20,
        };
        let reviews = latest_reviews(Some(&vec![approved, rejected_earlier, rejected_newer]));
        assert_eq!(reviews.len(), 2);
        assert_eq!(reviews[0].reviewer, "a");
        assert_eq!(reviews[0].decision, ReviewDecision::Approved);
        assert_eq!(reviews[1].decision, ReviewDecision::Rejected);
    }

    #[test]
    fn test_plan_trailer_parsing_is_case_insensitive() {
        let body = "context\n\nTest-Plan: do this\ntest-plan : and that\nOther: no\n";
        assert_eq!(test_plans(body), vec!["do this", "and that"]);
    }
}
