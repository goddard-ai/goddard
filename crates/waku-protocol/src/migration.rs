//! One-time copy of pre-Goddard state directories to their current names.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::identity::{
    DATA_DIRECTORY_NAME, HOME_DIRECTORY_NAME, LEGACY_DATA_DIRECTORY_NAME,
    LEGACY_HOME_DIRECTORY_NAME,
};

/// Written into a destination while its copy is in flight and left behind on
/// failure, so the next launch wipes the partial result and retries.
const MIGRATION_SENTINEL: &str = ".goddard-migration-incomplete";

pub struct MigrationReport {
    /// Destinations that received a fresh copy this launch.
    pub migrated: Vec<PathBuf>,
    pub failures: Vec<MigrationFailure>,
}

pub struct MigrationFailure {
    pub legacy: PathBuf,
    pub destination: PathBuf,
    pub error: io::Error,
}

impl MigrationReport {
    pub fn failed(&self) -> bool {
        !self.failures.is_empty()
    }
}

/// Copy `~/.waku` to `~/.goddard` and the platform data directories'
/// `Waku`/`Waku Debug` to the matching `Goddard` name. Each destination is
/// copied once — only while absent, or while still marked incomplete by a
/// failed attempt — and sources are never removed. Caches are not migrated;
/// they rebuild themselves.
pub fn migrate_legacy_directories() -> MigrationReport {
    let mut report = MigrationReport {
        migrated: Vec::new(),
        failures: Vec::new(),
    };
    let mut pairs: Vec<(PathBuf, PathBuf)> = Vec::new();
    if let Some(home) = dirs::home_dir() {
        pairs.push((
            home.join(LEGACY_HOME_DIRECTORY_NAME),
            home.join(HOME_DIRECTORY_NAME),
        ));
    }
    for base in [dirs::data_local_dir(), dirs::data_dir()]
        .into_iter()
        .flatten()
    {
        pairs.push((
            base.join(LEGACY_DATA_DIRECTORY_NAME),
            base.join(DATA_DIRECTORY_NAME),
        ));
    }
    pairs.sort();
    pairs.dedup();
    for (legacy, destination) in pairs {
        match migrate_directory(&legacy, &destination) {
            Ok(true) => report.migrated.push(destination),
            Ok(false) => {}
            Err(error) => report.failures.push(MigrationFailure {
                legacy,
                destination,
                error,
            }),
        }
    }
    report
}

fn migrate_directory(legacy: &Path, destination: &Path) -> io::Result<bool> {
    if !legacy.is_dir() {
        return Ok(false);
    }
    let sentinel = destination.join(MIGRATION_SENTINEL);
    match fs::read_dir(destination) {
        // A destination marked incomplete is a leftover partial copy — wipe
        // and retry. An empty destination has nothing to lose, so it also
        // migrates. Anything else counts as already settled.
        Ok(_) if sentinel.exists() => fs::remove_dir_all(destination)?,
        Ok(mut entries) => {
            if entries.next().is_some() {
                return Ok(false);
            }
            match fs::remove_dir(destination) {
                // A concurrent migrator may have claimed the empty directory.
                Ok(()) => {}
                Err(_) if !destination.exists() => {}
                Err(error) => return Err(error),
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    // Stage beside the destination and rename in one step: the destination
    // only ever appears complete, and a concurrent migrator's rename loses
    // instead of interleaving files with ours.
    let staging = destination.with_file_name(format!(
        ".{}.migrating-{}",
        destination
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "goddard".into()),
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&staging);
    fs::create_dir_all(&staging)?;
    fs::write(staging.join(MIGRATION_SENTINEL), b"")?;
    let result = copy_directory(legacy, &staging)
        .and_then(|_| fs::remove_file(staging.join(MIGRATION_SENTINEL)))
        .and_then(|_| fs::rename(&staging, destination));
    match result {
        Ok(()) => Ok(true),
        Err(error) => {
            let _ = fs::remove_dir_all(&staging);
            // Another process completed its own copy first — that wins.
            if destination.exists() {
                Ok(false)
            } else {
                Err(error)
            }
        }
    }
}

fn copy_directory(source: &Path, destination: &Path) -> io::Result<()> {
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let target = destination.join(entry.file_name());
        let metadata = fs::symlink_metadata(entry.path())?;
        if metadata.is_symlink() {
            copy_symlink(&entry.path(), &target)?;
        } else if metadata.is_dir() {
            fs::create_dir_all(&target)?;
            copy_directory(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

/// Recreate the link rather than copying its target: workspaces under
/// `projects/` can link into much larger trees that must not be duplicated.
#[cfg(unix)]
fn copy_symlink(source: &Path, target: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(fs::read_link(source)?, target)
}

/// Windows needs privileges to create symlinks; copying the referent keeps
/// the file usable, and a dangling link simply fails the copy into a retry.
#[cfg(not(unix))]
fn copy_symlink(source: &Path, target: &Path) -> io::Result<()> {
    fs::copy(source, target).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pair(root: &Path) -> (PathBuf, PathBuf) {
        (root.join(".waku"), root.join(".goddard"))
    }

    #[test]
    fn copies_once_when_destination_is_absent() {
        let root = std::env::temp_dir().join(format!("waku-migrate-{}", std::process::id()));
        let (legacy, destination) = pair(&root);
        fs::create_dir_all(legacy.join("projects/2026-08-08/chat")).unwrap();
        fs::write(legacy.join("settings.json"), b"{}").unwrap();
        fs::write(legacy.join("projects/2026-08-08/chat/notes"), b"hi").unwrap();

        assert!(migrate_directory(&legacy, &destination).unwrap());
        assert_eq!(
            fs::read(destination.join("projects/2026-08-08/chat/notes")).unwrap(),
            b"hi"
        );
        assert_eq!(fs::read(destination.join("settings.json")).unwrap(), b"{}");
        assert!(!destination.join(MIGRATION_SENTINEL).exists());

        // A second run is a no-op even when the source changed.
        fs::write(legacy.join("settings.json"), b"{\"x\":1}").unwrap();
        assert!(!migrate_directory(&legacy, &destination).unwrap());
        assert_eq!(fs::read(destination.join("settings.json")).unwrap(), b"{}");

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn retries_after_an_incomplete_copy() {
        let root = std::env::temp_dir().join(format!("waku-migrate-retry-{}", std::process::id()));
        let (legacy, destination) = pair(&root);
        fs::create_dir_all(&legacy).unwrap();
        fs::write(legacy.join("settings.json"), b"{}").unwrap();
        fs::create_dir_all(&destination).unwrap();
        fs::write(destination.join("stale-partial"), b"").unwrap();
        fs::write(destination.join(MIGRATION_SENTINEL), b"").unwrap();

        assert!(migrate_directory(&legacy, &destination).unwrap());
        assert!(!destination.join("stale-partial").exists());
        assert_eq!(fs::read(destination.join("settings.json")).unwrap(), b"{}");

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn skips_when_there_is_nothing_to_migrate() {
        let root = std::env::temp_dir().join(format!("waku-migrate-none-{}", std::process::id()));
        let (legacy, destination) = pair(&root);

        assert!(!migrate_directory(&legacy, &destination).unwrap());
        assert!(!destination.exists());
    }

    /// An unreadable source file fails the copy without leaving a partial
    /// destination — the staged copy is dropped, so the next launch retries.
    #[cfg(unix)]
    #[test]
    fn failed_copy_leaves_no_destination() {
        use std::os::unix::fs::PermissionsExt as _;

        let root = std::env::temp_dir().join(format!("waku-migrate-fail-{}", std::process::id()));
        let (legacy, destination) = pair(&root);
        fs::create_dir_all(&legacy).unwrap();
        let blocked = legacy.join("blocked");
        fs::write(&blocked, b"secret").unwrap();
        fs::set_permissions(&blocked, fs::Permissions::from_mode(0o000)).unwrap();

        assert!(migrate_directory(&legacy, &destination).is_err());
        assert!(!destination.exists());
        assert!(fs::read_dir(&root).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .contains("migrating")
        }));

        fs::set_permissions(&blocked, fs::Permissions::from_mode(0o644)).unwrap();
        fs::remove_dir_all(&root).unwrap();
    }
}
