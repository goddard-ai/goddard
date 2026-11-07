//! Explicit, commit-only project sync. Never commits, stashes, or resets.
use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use waku_share::projects::normalize_origin;

use crate::git_commit::{ensure_repository, git_stdout, git_success};

pub fn info(path: &Path) -> anyhow::Result<(String, String)> {
    ensure_repository(path)?;
    let origin = git_stdout(path, &["remote", "get-url", "origin"])?;
    let branch = git_stdout(path, &["symbolic-ref", "--quiet", "--short", "HEAD"])
        .context("sync requires a checked-out branch")?;
    Ok((origin.trim().to_owned(), branch.trim().to_owned()))
}

pub fn matching_project(paths: &[PathBuf], origin: &str) -> anyhow::Result<PathBuf> {
    let matches: Vec<_> = paths
        .iter()
        .filter(|path| {
            info(path).is_ok_and(|(candidate, _)| {
                normalize_origin(&candidate) == normalize_origin(origin)
            })
        })
        .collect();
    match matches.as_slice() {
        [path] => Ok((*path).clone()),
        [] => bail!("no project with the same origin on this host"),
        _ => bail!("multiple projects have the same origin on this host"),
    }
}

fn verify(path: &Path, origin: &str, branch: &str) -> anyhow::Result<()> {
    let (actual_origin, actual_branch) = info(path)?;
    if normalize_origin(&actual_origin) != normalize_origin(origin) || actual_branch != branch {
        bail!("project origin or checked-out branch changed; select the matching branch and retry");
    }
    Ok(())
}

pub fn push(path: &Path, origin: &str, branch: &str) -> anyhow::Result<()> {
    verify(path, origin, branch)?;
    let reference = format!("refs/heads/{branch}:refs/heads/{branch}");
    git_success(path, &["push", "origin", &reference])?;
    Ok(())
}

pub fn pull(path: &Path, origin: &str, branch: &str, check_only: bool) -> anyhow::Result<()> {
    verify(path, origin, branch)?;
    if !git_stdout(path, &["status", "--porcelain", "--untracked-files=normal"])?
        .trim()
        .is_empty()
    {
        bail!("destination working tree is dirty; left unchanged");
    }
    if crate::git_panel::sync_in_progress(path)?.is_some() {
        bail!("destination already has an integration in progress");
    }
    if !check_only {
        let reference = format!("refs/heads/{branch}");
        git_success(
            path,
            &[
                "-c",
                "pull.autostash=false",
                "-c",
                "merge.autostash=false",
                "pull",
                "--ff-only",
                "--no-rebase",
                "origin",
                &reference,
            ],
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Repositories(PathBuf);
    impl Drop for Repositories {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn git(path: &Path, args: &[&str]) -> String {
        git_stdout(path, args).unwrap()
    }

    #[test]
    fn sync_preserves_uncommitted_work_and_refuses_unsafe_destinations() {
        let root = Repositories(
            std::env::temp_dir().join(format!("project-sync-{}", uuid::Uuid::new_v4())),
        );
        std::fs::create_dir_all(&root.0).unwrap();
        let remote = root.0.join("origin.git");
        let source = root.0.join("source");
        let target = root.0.join("target");
        git(
            &root.0,
            &["init", "--bare", "-b", "main", remote.to_str().unwrap()],
        );
        git(
            &root.0,
            &["clone", remote.to_str().unwrap(), source.to_str().unwrap()],
        );
        git(&source, &["config", "user.email", "sync@example.com"]);
        git(&source, &["config", "user.name", "Sync Test"]);
        std::fs::write(source.join("file"), "base\n").unwrap();
        git(&source, &["add", "file"]);
        git(&source, &["commit", "-m", "base"]);
        let (origin, branch) = info(&source).unwrap();
        push(&source, &origin, &branch).unwrap();
        git(
            &root.0,
            &["clone", remote.to_str().unwrap(), target.to_str().unwrap()],
        );
        std::fs::write(source.join("file"), "committed\n").unwrap();
        git(&source, &["commit", "-am", "next"]);
        std::fs::write(source.join("file"), "uncommitted\n").unwrap();
        push(&source, &origin, &branch).unwrap();
        assert_eq!(
            std::fs::read_to_string(source.join("file")).unwrap(),
            "uncommitted\n"
        );
        std::fs::write(target.join("untracked"), "keep me").unwrap();
        let before = git(&target, &["rev-parse", "HEAD"]);
        assert!(
            pull(&target, &origin, &branch, false)
                .unwrap_err()
                .to_string()
                .contains("dirty")
        );
        assert_eq!(git(&target, &["rev-parse", "HEAD"]), before);
        assert_eq!(
            std::fs::read_to_string(target.join("untracked")).unwrap(),
            "keep me"
        );
        std::fs::remove_file(target.join("untracked")).unwrap();
        pull(&target, &origin, &branch, false).unwrap();
        assert_eq!(
            std::fs::read_to_string(target.join("file")).unwrap(),
            "committed\n"
        );
        assert_eq!(
            matching_project(&[target.clone()], &origin).unwrap(),
            target
        );
        assert!(matching_project(&[], &origin).is_err());
        assert!(matching_project(&[source.clone(), target.clone()], &origin).is_err());
        assert!(pull(&target, &origin, "different-branch", false).is_err());
        git(&target, &["config", "user.email", "sync@example.com"]);
        git(&target, &["config", "user.name", "Sync Test"]);
        std::fs::write(target.join("file"), "target commit\n").unwrap();
        git(&target, &["commit", "-am", "diverge target"]);
        git(&source, &["commit", "-am", "diverge source"]);
        push(&source, &origin, &branch).unwrap();
        let before = git(&target, &["rev-parse", "HEAD"]);
        assert!(pull(&target, &origin, &branch, false).is_err());
        assert_eq!(git(&target, &["rev-parse", "HEAD"]), before);
        assert!(
            crate::git_panel::sync_in_progress(&target)
                .unwrap()
                .is_none()
        );
    }
}
