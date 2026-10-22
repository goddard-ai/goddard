//! Single-commit QA checkouts. All Git work runs on the daemon worker.
use std::path::Path;

use anyhow::{Context as _, ensure};
use waku_protocol::git::ReviewDecision;
use waku_protocol::workspace::{QaCheckout, QaConfig};

use crate::git_commit::{git_capture, git_optional_stdout, git_stdout, git_success};

fn remote_default(cwd: &Path) -> anyhow::Result<String> {
    let refs = git_stdout(cwd, &["ls-remote", "--symref", "origin", "HEAD"])?;
    let branch = refs
        .lines()
        .find_map(|line| {
            line.strip_prefix("ref: refs/heads/")?
                .strip_suffix("\tHEAD")
        })
        .context("origin does not advertise a default branch")?;
    crate::review::qa_branch_checked(branch)
}

fn local_branch(cwd: &Path, branch: &str) -> bool {
    git_capture(
        cwd,
        &[
            "show-ref",
            "--verify",
            "--quiet",
            &format!("refs/heads/{branch}"),
        ],
    )
    .is_ok_and(|output| output.status.success())
}

pub fn defaults(cwd: &Path, preferred: &str) -> anyhow::Result<QaConfig> {
    let base = remote_default(cwd)?;
    let branch = [preferred, "qa", "dev"]
        .into_iter()
        .map(str::trim)
        .find(|branch| !branch.is_empty() && *branch != base && local_branch(cwd, branch))
        .unwrap_or(if base == "dev" { "qa" } else { "dev" })
        .to_owned();
    let package = std::fs::read_to_string(cwd.join("package.json"))
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok());
    let manager = if cwd.join("bun.lock").is_file() || cwd.join("bun.lockb").is_file() {
        "bun"
    } else if cwd.join("pnpm-lock.yaml").is_file() {
        "pnpm"
    } else if cwd.join("yarn.lock").is_file() {
        "yarn"
    } else {
        "npm"
    };
    let script = package
        .as_ref()
        .and_then(|package| package.get("scripts"))
        .and_then(|scripts| {
            ["dev", "start"]
                .into_iter()
                .find(|key| scripts.get(key).is_some_and(|v| v.is_string()))
        });
    Ok(QaConfig {
        branch,
        dev_command: script
            .map(|script| format!("{manager} run {script}"))
            .unwrap_or_default(),
        install_command: if package.is_some() {
            match manager {
                "npm" if cwd.join("package-lock.json").is_file() => "npm ci".into(),
                "pnpm" => "pnpm install --frozen-lockfile".into(),
                "bun" => "bun install --frozen-lockfile".into(),
                _ => format!("{manager} install"),
            }
        } else {
            String::new()
        },
    })
}

fn fetched_base(cwd: &Path) -> anyhow::Result<(String, String)> {
    let branch = remote_default(cwd)?;
    // Fetch this exact branch even when origin's configured refspec is narrow.
    git_success(
        cwd,
        &[
            "fetch",
            "origin",
            &format!("+refs/heads/{branch}:refs/remotes/origin/{branch}"),
        ],
    )?;
    let sha = git_stdout(
        cwd,
        &[
            "rev-parse",
            "--verify",
            &format!("refs/remotes/origin/{branch}"),
        ],
    )?;
    Ok((branch, sha.trim().to_owned()))
}

fn sole_parent(cwd: &Path, sha: &str, expected: &str) -> anyhow::Result<()> {
    ensure!(
        sha.len() >= 40 && sha.bytes().all(|b| b.is_ascii_hexdigit()),
        "invalid QA commit identity"
    );
    let parents = git_stdout(cwd, &["show", "-s", "--format=%P", sha])?;
    ensure!(
        parents.trim() == expected,
        "the QA commit must have exactly one parent: the reviewed default-branch tip"
    );
    Ok(())
}

pub fn start(cwd: &Path, branch: &str) -> anyhow::Result<QaCheckout> {
    ensure!(!branch.trim().is_empty(), "choose a QA branch");
    let branch = crate::review::qa_branch_checked(branch)?;
    let (base_branch, base_sha) = fetched_base(cwd)?;
    ensure!(
        branch != base_branch,
        "the QA branch cannot be the default branch"
    );
    ensure!(
        local_branch(cwd, &branch),
        "local QA branch {branch} does not exist"
    );
    let source = format!("refs/heads/{branch}");
    ensure!(
        git_capture(cwd, &["merge-base", "--is-ancestor", &base_sha, &source])?
            .status
            .success(),
        "{branch} has diverged from origin/{base_branch}; reconcile the branches before starting QA"
    );
    let commits = git_stdout(
        cwd,
        &[
            "rev-list",
            "--first-parent",
            "--reverse",
            &format!("{base_sha}..{source}"),
        ],
    )?;
    let sha = commits
        .lines()
        .next()
        .context("there are no commits waiting for QA")?;
    sole_parent(cwd, sha, &base_sha)?;
    let commit = crate::git_panel::commit(cwd, sha)?;
    let worktree = waku_git::worktree::create(cwd, None, Some(sha), false, &[])?;
    Ok(QaCheckout {
        base_branch,
        base_sha,
        qa_branch: branch,
        commit,
        worktree,
    })
}

/// Check the tested worktree, not another checkout of the project.
pub fn unchanged(cwd: &Path, checkout: &QaCheckout) -> anyhow::Result<()> {
    ensure!(
        std::fs::canonicalize(cwd)? == std::fs::canonicalize(&checkout.worktree.path)?,
        "QA worktree identity changed"
    );
    ensure!(
        git_stdout(cwd, &["rev-parse", "HEAD"])?.trim() == checkout.commit.sha,
        "the QA checkout's commit changed; start a fresh QA session"
    );
    ensure!(
        git_optional_stdout(cwd, &["symbolic-ref", "--quiet", "HEAD"])?.is_none(),
        "the QA checkout must remain detached"
    );
    ensure!(
        git_stdout(cwd, &["status", "--porcelain", "--untracked-files=all"])?
            .trim()
            .is_empty(),
        "the QA checkout has changes; submit corrections through the QA branch and test them in a fresh session"
    );
    sole_parent(cwd, &checkout.commit.sha, &checkout.base_sha)
}

pub fn promote(cwd: &Path, checkout: &QaCheckout) -> anyhow::Result<()> {
    unchanged(cwd, checkout)?;
    let (branch, base) = fetched_base(cwd)?;
    ensure!(
        branch == checkout.base_branch,
        "the remote default branch changed; start a fresh QA session"
    );
    // A lost push response can safely be retried without advancing anything else.
    if base == checkout.commit.sha {
        return Ok(());
    }
    ensure!(
        base == checkout.base_sha,
        "origin/{branch} moved since this QA session started; start a fresh QA session"
    );
    let qa_branch = crate::review::qa_branch_checked(&checkout.qa_branch)?;
    ensure!(
        qa_branch != branch,
        "the QA branch cannot be the default branch"
    );
    crate::review::record_on_ref(
        cwd,
        &checkout.commit.sha,
        ReviewDecision::Approved,
        &format!("refs/heads/{qa_branch}"),
    )?;
    unchanged(cwd, checkout)?;
    // The parent check proves this is a fast-forward. The explicit lease is
    // a compare-and-swap: even a concurrent rewind must not advance unnoticed.
    git_success(
        cwd,
        &[
            "push",
            &format!(
                "--force-with-lease=refs/heads/{branch}:{}",
                checkout.base_sha
            ),
            "origin",
            &format!("{}:refs/heads/{branch}", checkout.commit.sha),
        ],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, path::PathBuf};

    struct Fixture {
        root: PathBuf,
        repo: PathBuf,
        remote: PathBuf,
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }
    fn git(cwd: &Path, args: &[&str]) -> String {
        git_stdout(cwd, args).unwrap()
    }
    fn commit(cwd: &Path, name: &str) -> String {
        fs::write(cwd.join(name), name).unwrap();
        git(cwd, &["add", name]);
        git(cwd, &["commit", "-qm", name]);
        git(cwd, &["rev-parse", "HEAD"]).trim().to_owned()
    }
    fn fixture() -> Fixture {
        let root = std::env::temp_dir().join(format!("goddard-qa-{}", uuid::Uuid::new_v4()));
        let repo = root.join("repo");
        let remote = root.join("remote.git");
        fs::create_dir_all(&repo).unwrap();
        git(&repo, &["init", "-b", "main"]);
        git(&repo, &["config", "user.name", "QA Tester"]);
        git(&repo, &["config", "user.email", "qa@example.test"]);
        commit(&repo, "base");
        git(&repo, &["clone", "--bare", ".", remote.to_str().unwrap()]);
        git(
            &repo,
            &["remote", "add", "origin", remote.to_str().unwrap()],
        );
        git(&repo, &["checkout", "-b", "dev"]);
        Fixture { root, repo, remote }
    }
    #[test]
    fn local_queue_promotes_only_the_tested_commit_and_retries_safely() {
        let f = fixture();
        let first = commit(&f.repo, "first");
        commit(&f.repo, "later");
        let q = start(&f.repo, "dev").unwrap();
        assert_eq!(q.commit.sha, first);
        assert!(git(&q.worktree.path, &["branch", "--show-current"]).is_empty());
        assert!(!q.worktree.path.join("later").exists());
        promote(&q.worktree.path, &q).unwrap();
        promote(&q.worktree.path, &q).unwrap();
        assert_eq!(git(&f.remote, &["rev-parse", "main"]).trim(), first);
        assert!(git(&f.remote, &["notes", "--ref=qa", "show", &first]).contains("approved"));
        waku_git::worktree::remove(&q.worktree.path, false).unwrap();
    }
    #[test]
    fn dirty_checkout_and_moved_remote_never_promote() {
        let f = fixture();
        commit(&f.repo, "first");
        let q = start(&f.repo, "dev").unwrap();
        fs::write(q.worktree.path.join("untracked"), "fix").unwrap();
        assert!(
            promote(&q.worktree.path, &q)
                .unwrap_err()
                .to_string()
                .contains("has changes")
        );
        fs::remove_file(q.worktree.path.join("untracked")).unwrap();
        git(&f.repo, &["checkout", "main"]);
        let hotfix = commit(&f.repo, "hotfix");
        git(&f.repo, &["push", "origin", "main"]);
        assert!(
            promote(&q.worktree.path, &q)
                .unwrap_err()
                .to_string()
                .contains("moved")
        );
        assert_eq!(git(&f.remote, &["rev-parse", "main"]).trim(), hotfix);
        assert!(start(&f.repo, "dev").is_err());
        assert!(start(&f.repo, "main").is_err());
        waku_git::worktree::remove(&q.worktree.path, false).unwrap();
    }
    #[test]
    fn defaults_keep_commands_editable_and_never_choose_the_default_branch() {
        let f = fixture();
        fs::write(f.repo.join("package.json"), r#"{"scripts":{"dev":"vite"}}"#).unwrap();
        fs::write(f.repo.join("pnpm-lock.yaml"), "").unwrap();
        let config = defaults(&f.repo, "main").unwrap();
        assert_eq!(config.branch, "dev");
        assert_eq!(config.dev_command, "pnpm run dev");
        assert_eq!(config.install_command, "pnpm install --frozen-lockfile");
    }

    #[test]
    fn changed_head_and_attached_checkout_cannot_be_promoted() {
        let f = fixture();
        let original = commit(&f.repo, "first");
        let q = start(&f.repo, "dev").unwrap();
        let correction = commit(&q.worktree.path, "correction");
        assert!(
            promote(&q.worktree.path, &q)
                .unwrap_err()
                .to_string()
                .contains("commit changed")
        );
        // Retain the correction before exercising the detached-HEAD check.
        git(&f.repo, &["branch", "correction", &correction]);
        git(&q.worktree.path, &["checkout", "--detach", &original]);
        git(&q.worktree.path, &["checkout", "-b", "review"]);
        assert!(
            promote(&q.worktree.path, &q)
                .unwrap_err()
                .to_string()
                .contains("detached")
        );
        assert_eq!(git(&f.remote, &["rev-parse", "main"]).trim(), q.base_sha);
        waku_git::worktree::remove(&q.worktree.path, false).unwrap();
    }

    #[test]
    fn respects_a_non_main_default_and_rejects_merge_candidates() {
        let f = fixture();
        git(&f.repo, &["branch", "-m", "main", "trunk"]);
        git(&f.repo, &["push", "origin", "trunk"]);
        git(&f.remote, &["symbolic-ref", "HEAD", "refs/heads/trunk"]);
        let first = commit(&f.repo, "first");
        let q = start(&f.repo, "dev").unwrap();
        assert_eq!(q.base_branch, "trunk");
        promote(&q.worktree.path, &q).unwrap();
        assert_eq!(git(&f.remote, &["rev-parse", "trunk"]).trim(), first);
        assert!(
            start(&f.repo, "dev")
                .unwrap_err()
                .to_string()
                .contains("no commits")
        );
        waku_git::worktree::remove(&q.worktree.path, false).unwrap();

        git(&f.repo, &["checkout", "-b", "feature"]);
        commit(&f.repo, "feature");
        git(&f.repo, &["checkout", "dev"]);
        git(
            &f.repo,
            &["merge", "--no-ff", "feature", "-m", "merge candidate"],
        );
        assert!(
            start(&f.repo, "dev")
                .unwrap_err()
                .to_string()
                .contains("exactly one parent")
        );
    }
}
