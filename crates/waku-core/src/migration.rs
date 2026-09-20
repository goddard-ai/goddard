//! Itemized adoption of the legacy platform data directory (`Waku` →
//! `Goddard`, `Waku Debug` → `Goddard Debug`).
//!
//! This pair carries `app.db`, whose schema this crate owns: the database is
//! cloned through `VACUUM INTO`, and only when every migration it records is
//! one this build knows. An unfamiliar tag means the legacy install diverged
//! past us, so its database is left for a build that understands it rather
//! than upgraded into a shape we cannot read.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use rusqlite::Connection;
use waku_protocol::identity::{DATA_DIRECTORY_NAME, LEGACY_DATA_DIRECTORY_NAME};
use waku_protocol::migration::{
    MigrationReport, copy_file_if_absent, link_tree_entries, migration_temp_path,
    sweep_migration_artifacts,
};

use crate::persistence::MIGRATIONS;

/// Data-directory files copied whole.
const COPIED_FILES: &[&str] = &["settings.json", "state.json"];

/// Data-directory trees adopted per item. `blobs/` holds content referenced
/// by `app.db` rows; linking shares it with the legacy install instead of
/// duplicating it.
const LINKED_TREES: &[&str] = &["blobs"];

/// Adopt each platform data directory's `Waku`/`Waku Debug` under the
/// matching `Goddard` name. Sources are never removed.
pub fn migrate_data_directory() -> MigrationReport {
    let mut report = MigrationReport::default();
    let mut pairs: Vec<(PathBuf, PathBuf)> = [dirs::data_local_dir(), dirs::data_dir()]
        .into_iter()
        .flatten()
        .map(|base| {
            (
                base.join(LEGACY_DATA_DIRECTORY_NAME),
                base.join(DATA_DIRECTORY_NAME),
            )
        })
        .collect();
    pairs.sort();
    pairs.dedup();
    for (legacy, destination) in pairs {
        migrate_data_pair(&legacy, &destination, &mut report);
    }
    report
}

fn migrate_data_pair(legacy: &Path, destination: &Path, report: &mut MigrationReport) {
    sweep_migration_artifacts(destination);
    let Ok(legacy) = dunce::canonicalize(legacy) else {
        return;
    };
    if !legacy.is_dir() {
        return;
    }
    let destination =
        dunce::canonicalize(destination).unwrap_or_else(|_| destination.to_path_buf());
    if let Err(error) = fs::create_dir_all(&destination) {
        report.record_failure(legacy, destination, error);
        return;
    }
    let entries = match fs::read_dir(&legacy) {
        Ok(entries) => entries,
        Err(error) => {
            report.record_failure(legacy, destination, error);
            return;
        }
    };
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                report.record_failure(legacy.clone(), destination.clone(), error);
                continue;
            }
        };
        let source = entry.path();
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            report.record_skipped(source);
            continue;
        };
        let Ok(target) = dunce::canonicalize(&source) else {
            report.record_skipped(source);
            continue;
        };
        let destination_item = destination.join(&name);
        if name == "app.db" && target.is_file() {
            migrate_database(&target, &destination_item, report);
        } else if COPIED_FILES.contains(&name.as_str()) && target.is_file() {
            match copy_file_if_absent(&target, &destination_item) {
                Ok(true) => report.record_migrated(destination_item),
                Ok(false) => {}
                Err(error) => report.record_failure(source, destination_item, error),
            }
        } else if LINKED_TREES.contains(&name.as_str()) && target.is_dir() {
            link_tree_entries(&target, &destination_item, report);
        } else {
            // `Computer Use/` helper apps, `app.db` sidecars, and anything
            // unrecognized are left alone.
            report.record_skipped(source);
        }
    }
}

/// Clone `source` into `destination` through `VACUUM INTO`, which produces a
/// single consistent file — no `-wal`/`-shm` sidecars to coordinate — but
/// only after checking the source's recorded migrations are all ones this
/// build knows. Anything already at `destination` wins.
fn migrate_database(source: &Path, destination: &Path, report: &mut MigrationReport) {
    if fs::symlink_metadata(destination).is_ok() {
        return;
    }
    // Open with ordinary flags so a WAL-mode source is read through the usual
    // recovery path; VACUUM INTO takes the locks a consistent snapshot needs.
    let connection = match Connection::open(source) {
        Ok(connection) => connection,
        Err(error) => {
            report.record_failure(
                source.to_path_buf(),
                destination.to_path_buf(),
                io::Error::other(error.to_string()),
            );
            return;
        }
    };
    match database_is_compatible(&connection) {
        Ok(true) => {}
        Ok(false) => {
            report.record_skipped(source.to_path_buf());
            return;
        }
        Err(error) => {
            report.record_failure(source.to_path_buf(), destination.to_path_buf(), error);
            return;
        }
    }
    let temp = migration_temp_path(destination);
    let _ = fs::remove_file(&temp);
    let result = connection
        .execute("VACUUM INTO ?1", [&temp.to_string_lossy().into_owned()])
        .map_err(|error| io::Error::other(error.to_string()))
        .and_then(|_| fs::rename(&temp, destination));
    match result {
        Ok(()) => report.record_migrated(destination.to_path_buf()),
        Err(error) => {
            let _ = fs::remove_file(&temp);
            if fs::symlink_metadata(destination).is_err() {
                report.record_failure(source.to_path_buf(), destination.to_path_buf(), error);
            }
        }
    }
}

/// Whether `app.db` predates or diverges from this build's schema. A database
/// without a `migrations` table predates the migration framework — replaying
/// every migration onto it would collide with its existing tables — and one
/// recording an unknown tag came from a newer lineage. Both are skipped.
fn database_is_compatible(connection: &Connection) -> io::Result<bool> {
    let to_io = |error: rusqlite::Error| io::Error::other(error.to_string());
    let has_table: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'migrations')",
            [],
            |row| row.get(0),
        )
        .map_err(to_io)?;
    if !has_table {
        return Ok(false);
    }
    let mut statement = connection
        .prepare("SELECT tag FROM migrations")
        .map_err(to_io)?;
    let tags = statement
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(to_io)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(to_io)?;
    Ok(tags
        .iter()
        .all(|tag| MIGRATIONS.iter().any(|(known, _)| *known == tag)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::persistence::apply_migrations;

    fn test_root(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("waku-migrate-data-{name}-{}", std::process::id()))
    }

    fn pair(root: &Path) -> (PathBuf, PathBuf) {
        (root.join("Waku"), root.join("Goddard"))
    }

    #[test]
    fn adopts_the_database_and_small_state_files() {
        let root = test_root("items");
        let (legacy, destination) = pair(&root);
        fs::create_dir_all(legacy.join("blobs/sha")).unwrap();
        fs::create_dir_all(legacy.join("Computer Use")).unwrap();
        fs::write(legacy.join("settings.json"), b"{}").unwrap();
        fs::write(legacy.join("state.json"), b"{}").unwrap();
        fs::write(legacy.join("blobs/sha/blob"), b"blobby").unwrap();
        {
            let connection = Connection::open(legacy.join("app.db")).unwrap();
            assert!(apply_migrations(&connection).unwrap() > 0);
        }

        let mut report = MigrationReport::default();
        migrate_data_pair(&legacy, &destination, &mut report);
        assert!(report.failures.is_empty());

        // The cloned database is already at the latest schema.
        let connection = Connection::open(destination.join("app.db")).unwrap();
        assert_eq!(apply_migrations(&connection).unwrap(), 0);
        assert_eq!(fs::read(destination.join("settings.json")).unwrap(), b"{}");
        assert_eq!(fs::read(destination.join("state.json")).unwrap(), b"{}");
        assert_eq!(
            fs::read(destination.join("blobs/sha/blob")).unwrap(),
            b"blobby"
        );
        #[cfg(unix)]
        assert!(
            fs::symlink_metadata(destination.join("blobs/sha/blob"))
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert!(!destination.join("Computer Use").exists());
        assert!(
            report
                .skipped
                .iter()
                .any(|path| path.ends_with("Computer Use"))
        );

        // A second run is a no-op.
        let mut second = MigrationReport::default();
        migrate_data_pair(&legacy, &destination, &mut second);
        assert!(second.migrated.is_empty());
        assert!(second.failures.is_empty());

        drop(connection);
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn skips_a_database_from_a_newer_lineage() {
        let root = test_root("diverged");
        let (legacy, destination) = pair(&root);
        fs::create_dir_all(&legacy).unwrap();
        {
            let connection = Connection::open(legacy.join("app.db")).unwrap();
            apply_migrations(&connection).unwrap();
            connection
                .execute(
                    "INSERT INTO migrations(tag, applied_at) VALUES('9999_future', 0)",
                    [],
                )
                .unwrap();
        }

        let mut report = MigrationReport::default();
        migrate_data_pair(&legacy, &destination, &mut report);

        assert!(report.failures.is_empty());
        assert!(!destination.join("app.db").exists());
        assert!(report.skipped.iter().any(|path| path.ends_with("app.db")));

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn skips_a_database_from_before_migrations() {
        let root = test_root("premigration");
        let (legacy, destination) = pair(&root);
        fs::create_dir_all(&legacy).unwrap();
        Connection::open(legacy.join("app.db"))
            .unwrap()
            .execute_batch("CREATE TABLE stuff(id INTEGER)")
            .unwrap();

        let mut report = MigrationReport::default();
        migrate_data_pair(&legacy, &destination, &mut report);

        assert!(!destination.join("app.db").exists());
        assert!(report.skipped.iter().any(|path| path.ends_with("app.db")));

        fs::remove_dir_all(&root).unwrap();
    }
}
