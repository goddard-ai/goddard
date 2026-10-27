//! Cross-process admission for backends sharing one authoritative task store.
//!
//! Publication holds a short shared guard until its durable writer origin is
//! committed. Retirement excludes overlapping publication through its final
//! filesystem effect, then releases the guard. Quiet-writer evidence, archive
//! authority and shared catalog ownership remain separate preconditions.
use std::path::Path;
#[cfg(unix)]
use std::{fs, io};

use super::*;

pub(crate) struct WorkspaceAdmission {
    #[cfg(unix)]
    _locks: Vec<fs::File>,
}

impl WorkspaceAdmission {
    pub(super) fn writer(store: &Path, path: &Path) -> anyhow::Result<Self> {
        Self::acquire(store, path, false)
    }

    pub(super) fn retirement(store: &Path, path: &Path) -> anyhow::Result<Self> {
        Self::acquire(store, path, true)
    }

    #[cfg(not(unix))]
    fn acquire(_store: &Path, _path: &Path, retiring: bool) -> anyhow::Result<Self> {
        // Preserve provider availability where this gate is unsupported; never
        // infer destructive authority from unavailable host coordination.
        anyhow::ensure!(
            !retiring,
            "workspace retirement coordination is unavailable"
        );
        Ok(Self {})
    }

    #[cfg(unix)]
    fn acquire(store: &Path, path: &Path, retiring: bool) -> anyhow::Result<Self> {
        use sha2::{Digest, Sha256};
        use std::os::unix::{ffi::OsStrExt, fs::DirBuilderExt, fs::MetadataExt};

        let store = dunce::canonicalize(store)?;
        let path = dunce::canonicalize(path)?;
        let ancestors: Vec<_> = path.ancestors().take(129).collect();
        anyhow::ensure!(
            ancestors.len() <= 128,
            "workspace admission path is too deep"
        );
        let root = store
            .parent()
            .context("workspace admission store has no parent")?
            .join(format!(
                ".workspace-admission-{:x}",
                Sha256::digest(store.as_os_str().as_bytes())
            ));
        match fs::DirBuilder::new().mode(0o700).create(&root) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error.into()),
        }
        let metadata = fs::symlink_metadata(&root)?;
        // SAFETY: geteuid has no arguments or memory preconditions.
        let owner = unsafe { libc::geteuid() };
        anyhow::ensure!(
            metadata.is_dir() && metadata.uid() == owner && metadata.mode() & 0o077 == 0,
            "workspace admission directory is not private"
        );
        let lock_path = |path: &Path, kind: &str| {
            root.join(format!(
                "{:x}-{kind}",
                Sha256::digest(path.as_os_str().as_bytes())
            ))
        };
        let mut locks = Vec::with_capacity(ancestors.len() + 1);
        if retiring {
            // Exclusive own-path locks on ancestors conflict with a writer
            // rooted above us. The subtree lock conflicts with writers below.
            // Retirements serialize with each other, including sibling paths;
            // their optional heavy work already shares one host admission slot.
            for ancestor in &ancestors {
                locks.push(lock_file(&lock_path(ancestor, "own"), true)?);
            }
            locks.push(lock_file(&lock_path(&path, "subtree"), true)?);
        } else {
            locks.push(lock_file(&lock_path(&path, "own"), false)?);
            for ancestor in ancestors.iter().skip(1) {
                locks.push(lock_file(&lock_path(ancestor, "subtree"), false)?);
            }
        }
        Ok(Self { _locks: locks })
    }
}

#[cfg(unix)]
fn lock_file(path: &Path, exclusive: bool) -> anyhow::Result<fs::File> {
    use std::os::{
        fd::AsRawFd,
        unix::fs::{MetadataExt, OpenOptionsExt},
    };
    let file = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)?;
    let metadata = file.metadata()?;
    // SAFETY: geteuid has no arguments or memory preconditions.
    let owner = unsafe { libc::geteuid() };
    anyhow::ensure!(
        metadata.is_file()
            && metadata.uid() == owner
            && metadata.nlink() == 1
            && metadata.mode() & 0o077 == 0,
        "workspace admission lock is not a private regular file"
    );
    let mode = if exclusive {
        libc::LOCK_EX
    } else {
        libc::LOCK_SH
    };
    // SAFETY: the file owns the live descriptor. The nonblocking lock lasts
    // until this open file description closes; CLOEXEC excludes spawned tools.
    if unsafe { libc::flock(file.as_raw_fd(), mode | libc::LOCK_NB) } != 0 {
        let error = io::Error::last_os_error();
        if error.kind() == io::ErrorKind::WouldBlock {
            anyhow::bail!(
                "workspace admission deferred: an intersecting workspace operation is publishing or retiring"
            );
        }
        return Err(error.into());
    }
    // Never unlink lock files: another process may already hold their inode.
    Ok(file)
}

impl WakuBackend {
    pub(crate) fn admit_workspace_writer(&self, path: &Path) -> anyhow::Result<WorkspaceAdmission> {
        WorkspaceAdmission::writer(self.task_store.path(), path)
    }
}

#[cfg(all(test, unix))]
#[path = "tests/workspace_admission.rs"]
mod tests;
