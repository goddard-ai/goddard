//! The Git inputs that make a checkout's history badge reusable. Working-tree
//! dirt is deliberately absent: callers still inspect it on every request.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, bail};

use crate::git_commit::{
    checkout_only_subjects, complete_git_optional_stdout, complete_git_stdout,
};

const MAX_GRAPH_METADATA_BYTES: u64 = 1024 * 1024;

#[derive(Debug, Eq, Hash, PartialEq)]
pub struct CheckoutHistoryState {
    common_dir: PathBuf,
    git_dir: PathBuf,
    head: Option<String>,
    branch: Option<String>,
    has_remote: bool,
    refs: String,
    base: Option<(String, String)>,
    shallow: Option<Vec<u8>>,
    grafts: Option<Vec<u8>>,
}

impl CheckoutHistoryState {
    /// Read Git's identities instead of assuming `.git` is a directory. The
    /// common directory groups linked worktrees; each worktree's own Git
    /// directory and symbolic HEAD keep their branch exclusions separate.
    /// `None` means there is no configured remote or no HEAD commit: history
    /// is immediately zero and does not need a retained cache entry.
    pub fn read(cwd: &Path, recorded_base: Option<&str>) -> anyhow::Result<Option<Self>> {
        let has_remote = !complete_git_stdout(cwd, &["remote"])?.is_empty();
        if !has_remote {
            return Ok(None);
        }
        let head =
            complete_git_optional_stdout(cwd, &["rev-parse", "--verify", "--quiet", "HEAD"])?;
        if head.is_none() {
            return Ok(None);
        }
        let directories = complete_git_stdout(
            cwd,
            &[
                "rev-parse",
                "--path-format=absolute",
                "--git-common-dir",
                "--git-dir",
            ],
        )?;
        let mut directories = directories.lines();
        let common_dir = directories
            .next()
            .context("Git returned no common directory")?;
        let git_dir = directories
            .next()
            .context("Git returned no worktree directory")?;
        let common_dir = dunce::canonicalize(common_dir)
            .context("could not resolve the common Git directory")?;
        let git_dir =
            dunce::canonicalize(git_dir).context("could not resolve the worktree Git directory")?;
        let branch = complete_git_optional_stdout(cwd, &["symbolic-ref", "-q", "HEAD"])?;
        // Checkpoint refs cannot change checkout_only_subjects and must not
        // invalidate every cache entry whenever another task finishes a turn.
        // Replace refs and shallow/graft metadata can change reachability even
        // while ordinary branch tips stay fixed, so retain those inputs too.
        let refs = complete_git_stdout(
            cwd,
            &[
                "for-each-ref",
                "--sort=refname",
                "--format=%(refname)%00%(objectname)%00%(symref)",
                "refs/heads",
                "refs/tags",
                "refs/remotes",
                "refs/replace",
            ],
        )?;
        let base = if has_remote && head.is_some() {
            crate::git_panel::land_base(cwd, recorded_base)?
                .map(|base| {
                    let oid = complete_git_stdout(cwd, &["rev-parse", "--verify", &base])?;
                    Ok::<_, anyhow::Error>((base, oid))
                })
                .transpose()?
        } else {
            None
        };
        let shallow = graph_metadata(&common_dir.join("shallow"))?;
        let grafts = graph_metadata(&common_dir.join("info/grafts"))?;
        Ok(Some(Self {
            common_dir,
            git_dir,
            head,
            branch,
            has_remote,
            refs,
            base,
            shallow,
            grafts,
        }))
    }

    /// Keep cached storage bounded by bytes as well as entry count. Large
    /// repositories otherwise multiply their ref lists across many worktrees.
    pub fn retained_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.common_dir.as_os_str().len()
            + self.git_dir.as_os_str().len()
            + self.head.as_ref().map_or(0, String::capacity)
            + self.branch.as_ref().map_or(0, String::capacity)
            + self.refs.capacity()
            + self
                .base
                .as_ref()
                .map_or(0, |(name, oid)| name.capacity() + oid.capacity())
            + self.shallow.as_ref().map_or(0, Vec::capacity)
            + self.grafts.as_ref().map_or(0, Vec::capacity)
    }

    /// A count is publishable only if the ref-dependent inputs remain the
    /// ones the caller used to identify it. Archive previews do not call this
    /// cache-facing API and continue to inspect their full subject list.
    pub fn count_if_unchanged(
        &self,
        cwd: &Path,
        recorded_base: Option<&str>,
    ) -> anyhow::Result<u64> {
        let count = checkout_only_subjects(cwd, recorded_base)?.len() as u64;
        if Some(self) != Self::read(cwd, recorded_base)?.as_ref() {
            bail!("repository refs changed while inspecting checkout history; retry shortly");
        }
        Ok(count)
    }
}

fn graph_metadata(path: &Path) -> anyhow::Result<Option<Vec<u8>>> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).context("could not read Git graph metadata"),
    };
    let mut bytes = Vec::new();
    file.take(MAX_GRAPH_METADATA_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_GRAPH_METADATA_BYTES {
        bail!("Git graph metadata exceeded the capture limit");
    }
    Ok(Some(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    struct Repository(PathBuf);

    impl Repository {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!("waku-history-{}", Uuid::new_v4()));
            fs::create_dir_all(&root).unwrap();
            git(&root, &["init", "-b", "main"]);
            git(&root, &["config", "user.name", "History Test"]);
            git(&root, &["config", "user.email", "history@example.com"]);
            git(&root, &["commit", "--allow-empty", "-m", "base"]);
            git(
                &root,
                &["remote", "add", "origin", "https://example.com/repo.git"],
            );
            git(&root, &["update-ref", "refs/remotes/origin/main", "HEAD"]);
            git(&root, &["checkout", "-b", "session"]);
            git(&root, &["commit", "--allow-empty", "-m", "session work"]);
            Self(root)
        }
    }

    impl Drop for Repository {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn git(cwd: &Path, args: &[&str]) {
        let output = crate::command_env::search_path_command("git")
            .args(args)
            .current_dir(cwd)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn history_inputs_follow_safety_refs_but_ignore_checkpoint_refs() {
        let repo = Repository::new();
        let initial = CheckoutHistoryState::read(&repo.0, None).unwrap().unwrap();
        assert_eq!(initial.count_if_unchanged(&repo.0, None).unwrap(), 1);
        git(&repo.0, &["update-ref", "refs/waku/test", "HEAD"]);
        assert_eq!(
            initial,
            CheckoutHistoryState::read(&repo.0, None).unwrap().unwrap()
        );

        for reference in [
            "refs/heads/saved",
            "refs/tags/saved",
            "refs/remotes/origin/saved",
        ] {
            git(&repo.0, &["update-ref", reference, "HEAD"]);
            let saved = CheckoutHistoryState::read(&repo.0, None).unwrap().unwrap();
            assert_ne!(initial, saved);
            assert_eq!(saved.count_if_unchanged(&repo.0, None).unwrap(), 0);
            assert!(initial.count_if_unchanged(&repo.0, None).is_err());
            git(&repo.0, &["update-ref", "-d", reference]);
            assert_eq!(
                initial,
                CheckoutHistoryState::read(&repo.0, None).unwrap().unwrap()
            );
        }

        git(&repo.0, &["remote", "remove", "origin"]);
        assert!(CheckoutHistoryState::read(&repo.0, None).unwrap().is_none());
    }

    #[test]
    fn symbolic_head_and_base_selection_change_even_when_commit_ids_do_not() {
        let repo = Repository::new();
        git(&repo.0, &["branch", "dev", "main"]);
        let attached = CheckoutHistoryState::read(&repo.0, None).unwrap().unwrap();
        git(&repo.0, &["checkout", "--detach", "HEAD"]);
        let detached = CheckoutHistoryState::read(&repo.0, None).unwrap().unwrap();
        assert_eq!(attached.head, detached.head);
        assert_ne!(attached, detached);

        let main = CheckoutHistoryState::read(&repo.0, Some("main"))
            .unwrap()
            .unwrap();
        let dev = CheckoutHistoryState::read(&repo.0, Some("dev"))
            .unwrap()
            .unwrap();
        assert_ne!(main, dev);
        assert_eq!(main.base.as_ref().unwrap().1, dev.base.as_ref().unwrap().1);

        git(&repo.0, &["update-ref", "refs/remotes/origin/dev", "main"]);
        git(
            &repo.0,
            &[
                "symbolic-ref",
                "refs/remotes/origin/HEAD",
                "refs/remotes/origin/main",
            ],
        );
        let main = CheckoutHistoryState::read(&repo.0, None).unwrap().unwrap();
        git(
            &repo.0,
            &[
                "symbolic-ref",
                "refs/remotes/origin/HEAD",
                "refs/remotes/origin/dev",
            ],
        );
        let dev = CheckoutHistoryState::read(&repo.0, None).unwrap().unwrap();
        assert_ne!(main, dev);
        assert_eq!(dev.base.as_ref().unwrap().0, "dev");
    }

    #[test]
    fn linked_worktrees_keep_separate_heads_and_track_the_primary_base() {
        let repo = Repository::new();
        git(&repo.0, &["branch", "dev", "main"]);
        let linked = repo.0.join("linked");
        git(
            &repo.0,
            &[
                "worktree",
                "add",
                "--detach",
                linked.to_str().unwrap(),
                "main",
            ],
        );
        let primary = CheckoutHistoryState::read(&repo.0, None).unwrap().unwrap();
        let before = CheckoutHistoryState::read(&linked, None).unwrap().unwrap();
        assert_eq!(primary.common_dir, before.common_dir);
        assert_ne!(primary.git_dir, before.git_dir);
        assert_ne!(primary.head, before.head);
        assert_eq!(before.base.as_ref().unwrap().0, "session");

        git(&repo.0, &["checkout", "dev"]);
        let after = CheckoutHistoryState::read(&linked, None).unwrap().unwrap();
        assert_eq!(before.refs, after.refs);
        assert_eq!(before.head, after.head);
        assert_ne!(before, after);
        assert_eq!(after.base.as_ref().unwrap().0, "dev");
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_checkout_paths_share_the_canonical_identity() {
        let repo = Repository::new();
        let alias = repo.0.join("alias");
        std::os::unix::fs::symlink(&repo.0, &alias).unwrap();
        assert_eq!(
            CheckoutHistoryState::read(&repo.0, None).unwrap().unwrap(),
            CheckoutHistoryState::read(&alias, None).unwrap().unwrap(),
        );
    }
}
