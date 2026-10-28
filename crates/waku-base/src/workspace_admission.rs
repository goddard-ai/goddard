//! Cross-process admission for backends sharing one authoritative task store.
//!
//! Publication holds a short shared guard until its durable writer origin is
//! committed. Retirement excludes overlapping publication through its final
//! filesystem effect, then releases the guard. Quiet-writer evidence, archive
//! authority and shared catalog ownership remain separate preconditions.
#[cfg(unix)]
use anyhow::Context as _;
use std::path::{Path, PathBuf};
#[cfg(unix)]
use std::{fs, io};

pub struct WorkspaceAdmission {
    #[cfg(unix)]
    _locks: Vec<fs::File>,
}

#[cfg(unix)]
#[derive(Clone, Copy, PartialEq, Eq)]
enum AdmissionKind {
    Writer,
    Retirement,
    Materialization,
    Catalog,
}

impl WorkspaceAdmission {
    pub fn writer(store: &Path, path: &Path) -> anyhow::Result<Self> {
        Self::acquire(store, path, false)
    }

    pub fn retirement(store: &Path, path: &Path) -> anyhow::Result<Self> {
        Self::acquire(store, path, true)
    }

    /// Protect idempotent namespace creation, including an absent path, while
    /// allowing other creators to ensure the same parent directory.
    pub fn namespace(store: &Path, path: &Path) -> anyhow::Result<Self> {
        #[cfg(unix)]
        {
            Self::acquire_canonical(store, &catalog_path(path)?, AdmissionKind::Writer)
        }
        #[cfg(not(unix))]
        {
            let _ = (store, path);
            Ok(Self {})
        }
    }

    /// Serialize creation/restoration of one destination, and keep descendant
    /// writer/catalog admission out until the directory is fully materialized.
    pub fn materialize(store: &Path, path: &Path) -> anyhow::Result<Self> {
        #[cfg(unix)]
        {
            Self::acquire_canonical(store, &catalog_path(path)?, AdmissionKind::Materialization)
        }
        #[cfg(not(unix))]
        {
            let _ = (store, path);
            Ok(Self {})
        }
    }

    /// Guard catalog ownership transitions, including paths not materialized
    /// yet. One common ancestor bounds descriptor use for large transactions.
    pub fn catalog(store: &Path, paths: &[PathBuf]) -> anyhow::Result<Option<Self>> {
        #[cfg(not(unix))]
        {
            let _ = (store, paths);
            Ok(None)
        }
        #[cfg(unix)]
        {
            let Some(first) = paths.first() else {
                return Ok(None);
            };
            let mut common = catalog_path(first)?;
            for path in &paths[1..] {
                let path = catalog_path(path)?;
                while !path.starts_with(&common) {
                    anyhow::ensure!(
                        common.pop(),
                        "catalog workspace paths have no common ancestor"
                    );
                }
            }
            Ok(Some(Self::acquire_canonical(
                store,
                &common,
                AdmissionKind::Catalog,
            )?))
        }
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
        Self::acquire_canonical(
            store,
            &dunce::canonicalize(path)?,
            if retiring {
                AdmissionKind::Retirement
            } else {
                AdmissionKind::Writer
            },
        )
    }

    #[cfg(unix)]
    fn acquire_canonical(store: &Path, path: &Path, kind: AdmissionKind) -> anyhow::Result<Self> {
        use sha2::{Digest, Sha256};
        use std::os::unix::{ffi::OsStrExt, fs::DirBuilderExt, fs::MetadataExt};

        let store = dunce::canonicalize(store)?;
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
        let mut locks = Vec::with_capacity(2 * ancestors.len() + 1);
        if kind == AdmissionKind::Retirement {
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
            // An exclusive destination lease also excludes descendant writers
            // and overlapping allocators. Ancestor leases stay shared, so
            // different destinations can materialize concurrently.
            for (depth, ancestor) in ancestors.iter().enumerate() {
                locks.push(lock_file(
                    &lock_path(ancestor, "materialization"),
                    depth == 0 && kind == AdmissionKind::Materialization,
                )?);
            }
            match kind {
                // A coalesced catalog scope must exclude materialization below
                // it without excluding ordinary writer publication there.
                AdmissionKind::Catalog => locks.push(lock_file(&lock_path(path, "catalog"), true)?),
                AdmissionKind::Materialization => {
                    for ancestor in &ancestors {
                        locks.push(lock_file(&lock_path(ancestor, "catalog"), false)?);
                    }
                }
                _ => {}
            }
        }
        Ok(Self { _locks: locks })
    }
}

/// Resolve the nearest existing ancestor without treating a dangling symlink
/// as a missing ordinary directory. The suffix may contain only normal names.
#[cfg(unix)]
/// Resolve existing aliases and missing suffixes using the same bounded rules
/// as catalog admission. Call only on workers, before comparing physical owners.
pub fn catalog_path(path: &Path) -> anyhow::Result<PathBuf> {
    let absolute = std::path::absolute(path)?;
    let mut existing = absolute.as_path();
    let mut suffix = Vec::new();
    let mut resolved = loop {
        match dunce::canonicalize(existing) {
            Ok(path) => break path,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                anyhow::ensure!(
                    fs::symlink_metadata(existing).is_err(),
                    "catalog workspace alias is unresolved"
                );
                anyhow::ensure!(suffix.len() < 128, "catalog workspace path is too deep");
                let name = existing
                    .file_name()
                    .context("catalog workspace path cannot be resolved")?;
                suffix.push(name);
                existing = existing
                    .parent()
                    .context("catalog workspace path has no existing ancestor")?;
            }
            Err(error) => return Err(error.into()),
        }
    };
    for name in suffix.into_iter().rev() {
        resolved.push(name);
    }
    Ok(resolved)
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
            return Err(io::Error::new(io::ErrorKind::WouldBlock,
                "workspace admission deferred: an intersecting workspace operation is publishing or retiring").into());
        }
        return Err(error.into());
    }
    // Never unlink lock files: another process may already hold their inode.
    Ok(file)
}

#[cfg(all(test, unix))]
#[path = "workspace_admission_tests.rs"]
mod tests;
