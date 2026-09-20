//! Itemized adoption of the pre-Goddard `~/.waku` directory under `~/.goddard`.
//!
//! Migration is per item rather than a whole-tree copy: small app files are
//! copied atomically, while user-owned trees — workspaces, worktrees, and
//! archives, which can be arbitrarily large and stay shared with the legacy
//! app — are linked entry-by-entry into real destination directories. Items
//! already present at the destination are never overwritten, so a launch
//! killed mid-migration resumes where it stopped and a concurrent migrator
//! loses individual entries instead of interleaving files with ours.
//!
//! Anything not named here is left alone. Adopting a legacy item takes an
//! explicit rule so that a Waku layout this build no longer understands can
//! never leak into `~/.goddard`.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::identity::{
    DATA_DIRECTORY_NAME, HOME_DIRECTORY_NAME, LEGACY_DATA_DIRECTORY_NAME,
    LEGACY_HOME_DIRECTORY_NAME,
};

/// In-flight file copies are named `.<item>.migrating-<pid>` beside their
/// destination; older builds staged the whole tree under the same convention.
/// The pid lets a later launch sweep only a dead migrator's leftovers.
const MIGRATING_INFIX: &str = ".migrating-";

/// Older builds wrote this inside a staged copy to mark it incomplete. The
/// itemized pass never produces one, so any survivor is garbage.
const LEGACY_SENTINEL: &str = ".goddard-migration-incomplete";

/// Home-directory files copied whole. `settings.json` keeps its old name on
/// purpose: the settings reader folds it into `app.json` on first save.
const COPIED_FILES: &[&str] = &["settings.json", "app.json"];

/// Home-directory trees adopted per contained item — `projects/<date>/<slug>`,
/// `worktrees/<id>/<name>`, `archives/<date>/<slug>.zip`. Each first-level
/// entry is recreated as a real directory and each second-level entry becomes
/// a link, so items the app creates later never write through into Waku's
/// tree. Non-directory first-level entries link directly.
const LINKED_TREES: &[&str] = &["projects", "worktrees", "archives"];

#[derive(Default)]
pub struct MigrationReport {
    /// Destination paths that gained an adopted item this launch.
    pub migrated: Vec<PathBuf>,
    /// Legacy items deliberately left alone — unrecognized, or a shape this
    /// build knows it cannot adopt safely.
    pub skipped: Vec<PathBuf>,
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

    pub fn extend(&mut self, other: MigrationReport) {
        self.migrated.extend(other.migrated);
        self.skipped.extend(other.skipped);
        self.failures.extend(other.failures);
    }

    pub fn record_migrated(&mut self, destination: PathBuf) {
        self.migrated.push(destination);
    }

    pub fn record_skipped(&mut self, legacy: PathBuf) {
        self.skipped.push(legacy);
    }

    pub fn record_failure(&mut self, legacy: PathBuf, destination: PathBuf, error: io::Error) {
        self.failures.push(MigrationFailure {
            legacy,
            destination,
            error,
        });
    }
}

/// Adopt `~/.waku` under `~/.goddard`. The home pair carries no database —
/// `app.db` lives in the platform data directory, which `waku_core` migrates
/// because it owns that schema — so this stays filesystem-only.
pub fn migrate_home_directory() -> MigrationReport {
    let mut report = MigrationReport::default();
    if let Some(home) = dirs::home_dir() {
        migrate_home(
            &home.join(LEGACY_HOME_DIRECTORY_NAME),
            &home.join(HOME_DIRECTORY_NAME),
            &mut report,
        );
    }
    report
}

fn migrate_home(legacy: &Path, destination: &Path, report: &mut MigrationReport) {
    // Sweep even when nothing migrates: earlier builds leaked whole-directory
    // staging beside the destination, and that debris is worth removing on
    // every launch.
    sweep_migration_artifacts(destination);
    // Resolve once so adopted links point at the canonical location and keep
    // working if `legacy` — itself possibly a symlink — is later removed.
    let Ok(legacy) = fs::canonicalize(legacy) else {
        return;
    };
    if !legacy.is_dir() {
        return;
    }
    // A symlinked destination was relocated by the user; adopt items inside
    // its target rather than replacing the link.
    let destination = fs::canonicalize(destination).unwrap_or_else(|_| destination.to_path_buf());
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
        // Canonicalize each entry: links inside the legacy tree collapse to
        // their referent, and a dangling link simply skips.
        let Ok(target) = fs::canonicalize(&source) else {
            report.record_skipped(source);
            continue;
        };
        if COPIED_FILES.contains(&name.as_str()) && target.is_file() {
            match copy_file_if_absent(&target, &destination.join(&name)) {
                Ok(true) => report.record_migrated(destination.join(&name)),
                Ok(false) => {}
                Err(error) => report.record_failure(source, destination.join(&name), error),
            }
        } else if LINKED_TREES.contains(&name.as_str()) && target.is_dir() {
            link_tree_entries(&target, &destination.join(&name), report);
        } else {
            // Pre-`projects/` dated workspaces at the root stay where they
            // are — `projectless` still recognizes them under `~/.waku`, and
            // linking them under `~/.goddard` would let the daemon move them
            // out of the shared tree.
            report.record_skipped(source);
        }
    }
}

/// Copy `source` to `destination` unless anything — including a dangling
/// link — already claims it. The copy lands on a `.<name>.migrating-<pid>`
/// sibling and is renamed over, so readers only ever see the complete file.
pub fn copy_file_if_absent(source: &Path, destination: &Path) -> io::Result<bool> {
    if fs::symlink_metadata(destination).is_ok() {
        return Ok(false);
    }
    let temp = migration_temp_path(destination);
    let _ = fs::remove_file(&temp);
    fs::copy(source, &temp)?;
    match fs::rename(&temp, destination) {
        Ok(()) => Ok(true),
        Err(error) => {
            let _ = fs::remove_file(&temp);
            // A concurrent migrator's copy won the rename — that is fine.
            if fs::symlink_metadata(destination).is_ok() {
                Ok(false)
            } else {
                Err(error)
            }
        }
    }
}

/// The sibling an in-flight copy of `destination` is staged at.
pub fn migration_temp_path(destination: &Path) -> PathBuf {
    destination.with_file_name(format!(
        ".{}.migrating-{}",
        destination
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "goddard".into()),
        std::process::id()
    ))
}

/// Adopt `source`'s contents under `destination` one group at a time: each
/// first-level entry is recreated as a real directory and its children become
/// links, so directories the app reuses later (a `projects` date, a
/// `worktrees` namespace) never write through into the legacy tree. Entries
/// that are not directories link directly.
pub fn link_tree_entries(source: &Path, destination: &Path, report: &mut MigrationReport) {
    // A symlinked destination was relocated by the user; adopt inside its
    // target rather than beside it.
    let destination = fs::canonicalize(destination).unwrap_or_else(|_| destination.to_path_buf());
    if let Err(error) = fs::create_dir_all(&destination) {
        report.record_failure(source.to_path_buf(), destination, error);
        return;
    }
    let entries = match fs::read_dir(source) {
        Ok(entries) => entries,
        Err(error) => {
            report.record_failure(source.to_path_buf(), destination, error);
            return;
        }
    };
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                report.record_failure(source.to_path_buf(), destination.clone(), error);
                continue;
            }
        };
        let source_entry = entry.path();
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            report.record_skipped(source_entry);
            continue;
        };
        let Ok(target) = fs::canonicalize(&source_entry) else {
            report.record_skipped(source_entry);
            continue;
        };
        let destination_entry = destination.join(&name);
        if target.is_dir() {
            if let Err(error) = fs::create_dir_all(&destination_entry) {
                report.record_failure(source_entry, destination_entry, error);
                continue;
            }
            let sub_entries = match fs::read_dir(&target) {
                Ok(sub_entries) => sub_entries,
                Err(error) => {
                    report.record_failure(source_entry, destination_entry, error);
                    continue;
                }
            };
            for sub_entry in sub_entries {
                let sub_entry = match sub_entry {
                    Ok(sub_entry) => sub_entry,
                    Err(error) => {
                        report.record_failure(
                            source_entry.clone(),
                            destination_entry.clone(),
                            error,
                        );
                        continue;
                    }
                };
                let sub_source = sub_entry.path();
                let Some(sub_name) = sub_entry.file_name().to_str().map(str::to_owned) else {
                    report.record_skipped(sub_source);
                    continue;
                };
                let Ok(sub_target) = fs::canonicalize(&sub_source) else {
                    report.record_skipped(sub_source);
                    continue;
                };
                let sub_destination = destination_entry.join(&sub_name);
                match link_entry(&sub_target, &sub_destination) {
                    Ok(true) => report.record_migrated(sub_destination),
                    Ok(false) => {}
                    Err(error) => report.record_failure(sub_source, sub_destination, error),
                }
            }
        } else {
            match link_entry(&target, &destination_entry) {
                Ok(true) => report.record_migrated(destination_entry),
                Ok(false) => {}
                Err(error) => report.record_failure(source_entry, destination_entry, error),
            }
        }
    }
}

/// Link `destination` at `target`. `AlreadyExists` means the entry was
/// adopted by an earlier launch or a concurrent migrator — never an error,
/// and never a clobber.
pub fn link_entry(target: &Path, destination: &Path) -> io::Result<bool> {
    match create_link(target, destination) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Ok(false),
        Err(error) => Err(error),
    }
}

#[cfg(unix)]
fn create_link(target: &Path, destination: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(target, destination)
}

/// Windows creates directory and file links through different calls; picking
/// by the resolved target keeps a symlinked legacy entry the right kind.
#[cfg(windows)]
fn create_link(target: &Path, destination: &Path) -> io::Result<()> {
    if target.is_dir() {
        std::os::windows::fs::symlink_dir(target, destination)
    } else {
        std::os::windows::fs::symlink_file(target, destination)
    }
}

#[cfg(not(any(unix, windows)))]
fn create_link(_target: &Path, _destination: &Path) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "cannot link a legacy entry on this platform",
    ))
}

/// Directory names earlier builds could have staged beside `~/.goddard`.
/// Only these are recognized in the home root, so an unrelated
/// `.<item>.migrating-<pid>` entry another tool created is never touched
/// outside our own tree.
const SWEPT_SIBLING_NAMES: &[&str] = &[
    HOME_DIRECTORY_NAME,
    LEGACY_HOME_DIRECTORY_NAME,
    DATA_DIRECTORY_NAME,
    LEGACY_DATA_DIRECTORY_NAME,
];

/// Remove `.….migrating-<pid>` artifacts beside and inside `destination` whose
/// owning process is gone. Earlier builds staged the entire copy in a sibling
/// directory; current builds stage individual file copies — both use the same
/// convention, so one sweep covers every era. The sibling sweep only accepts
/// the recognized directory names above; inside `destination` itself any
/// `.<item>.migrating-<pid>` entry is ours. A live pid's artifacts belong to
/// a migrator that may still be running and are left alone.
pub fn sweep_migration_artifacts(destination: &Path) {
    if let Some(parent) = destination.parent() {
        sweep_directory(parent, Some(SWEPT_SIBLING_NAMES));
    }
    sweep_directory(destination, None);
    let _ = fs::remove_file(destination.join(LEGACY_SENTINEL));
}

fn sweep_directory(directory: &Path, allowed_items: Option<&[&str]>) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if is_migration_artifact(&name, allowed_items) {
            remove_any(&entry.path());
        }
    }
}

fn is_migration_artifact(name: &str, allowed_items: Option<&[&str]>) -> bool {
    if !name.starts_with('.') {
        return false;
    }
    let Some((item, pid)) = name.rsplit_once(MIGRATING_INFIX) else {
        return false;
    };
    if item.is_empty() {
        return false;
    }
    // In directories we do not own, only names this family of builds could
    // have produced count — anything else matching the pattern stays put.
    // Compare bare names: the extracted item keeps the artifact's leading
    // dot while the identity constants may or may not carry one.
    if let Some(items) = allowed_items {
        let bare = item.trim_start_matches('.');
        if !items
            .iter()
            .any(|known| known.trim_start_matches('.') == bare)
        {
            return false;
        }
    }
    let Ok(pid) = pid.parse::<u32>() else {
        return false;
    };
    !crate::pid::is_alive(pid)
}

fn remove_any(path: &Path) {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return;
    };
    let _ = if metadata.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_root(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("waku-migrate-{name}-{}", std::process::id()))
    }

    fn pair(root: &Path) -> (PathBuf, PathBuf) {
        (root.join(".waku"), root.join(".goddard"))
    }

    #[cfg(unix)]
    #[test]
    fn adopts_items_per_policy_and_stops_afterwards() {
        let root = test_root("items");
        let (legacy, destination) = pair(&root);
        fs::create_dir_all(legacy.join("projects/2026-08-08/chat")).unwrap();
        fs::create_dir_all(legacy.join("worktrees/abc123/wt")).unwrap();
        fs::create_dir_all(legacy.join("archives/2026-08-08")).unwrap();
        fs::create_dir_all(legacy.join("unrelated")).unwrap();
        fs::write(legacy.join("projects/2026-08-08/chat/notes"), b"hi").unwrap();
        fs::write(legacy.join("worktrees/abc123/wt/code.rs"), b"fn main() {}").unwrap();
        fs::write(legacy.join("archives/2026-08-08/chat.zip"), b"zip").unwrap();
        fs::write(legacy.join("archives/loose.zip"), b"zip").unwrap();
        fs::write(legacy.join("settings.json"), b"{}").unwrap();
        fs::write(legacy.join("app.json"), b"{\"a\":1}").unwrap();
        fs::write(legacy.join("app.db"), b"not-adopted").unwrap();
        fs::write(legacy.join(".DS_Store"), b"junk").unwrap();

        let mut report = MigrationReport::default();
        migrate_home(&legacy, &destination, &mut report);
        assert!(report.failures.is_empty(), "{:?}", report.failures.len());

        // Small app files are copied; their content crosses over.
        assert_eq!(fs::read(destination.join("settings.json")).unwrap(), b"{}");
        assert_eq!(
            fs::read(destination.join("app.json")).unwrap(),
            b"{\"a\":1}"
        );
        assert!(
            !fs::symlink_metadata(destination.join("settings.json"))
                .unwrap()
                .file_type()
                .is_symlink()
        );

        // Tree contents are linked per item, leaving real directories behind.
        let project = destination.join("projects/2026-08-08/chat");
        assert!(
            fs::symlink_metadata(&project)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(fs::read(project.join("notes")).unwrap(), b"hi");
        assert!(
            !fs::symlink_metadata(destination.join("projects/2026-08-08"))
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert!(
            fs::symlink_metadata(destination.join("worktrees/abc123/wt"))
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert!(
            fs::symlink_metadata(destination.join("archives/2026-08-08/chat.zip"))
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert!(
            fs::symlink_metadata(destination.join("archives/loose.zip"))
                .unwrap()
                .file_type()
                .is_symlink()
        );

        // The whitelist leaves everything else alone.
        assert!(!destination.join("app.db").exists());
        assert!(!destination.join(".DS_Store").exists());
        assert!(!destination.join("unrelated").exists());
        assert!(report.skipped.iter().any(|path| path.ends_with("app.db")));

        // A second run is a no-op.
        let mut second = MigrationReport::default();
        migrate_home(&legacy, &destination, &mut second);
        assert!(second.migrated.is_empty());
        assert!(second.failures.is_empty());

        fs::remove_dir_all(&root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn links_through_a_symlinked_legacy_root() {
        let root = test_root("symlinked");
        let real = root.join("elsewhere/.waku");
        let (legacy, destination) = pair(&root);
        fs::create_dir_all(real.join("projects/2026-08-08/chat")).unwrap();
        fs::write(real.join("settings.json"), b"{}").unwrap();
        std::os::unix::fs::symlink(&real, &legacy).unwrap();

        let mut report = MigrationReport::default();
        migrate_home(&legacy, &destination, &mut report);
        assert!(report.failures.is_empty());

        // The link points at the canonical target, so it survives the
        // `~/.waku` symlink itself being removed later.
        let link = destination.join("projects/2026-08-08/chat");
        assert_eq!(
            fs::read_link(&link).unwrap(),
            fs::canonicalize(&real)
                .unwrap()
                .join("projects/2026-08-08/chat")
        );
        fs::remove_file(&legacy).unwrap();
        assert!(link.is_dir());

        fs::remove_dir_all(&root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn never_overwrites_destination_items() {
        let root = test_root("settled");
        let (legacy, destination) = pair(&root);
        fs::create_dir_all(legacy.join("projects/2026-08-08/chat")).unwrap();
        fs::write(legacy.join("settings.json"), b"legacy").unwrap();
        fs::create_dir_all(destination.join("projects/2026-08-08/chat")).unwrap();
        fs::write(destination.join("settings.json"), b"current").unwrap();

        let mut report = MigrationReport::default();
        migrate_home(&legacy, &destination, &mut report);

        assert_eq!(
            fs::read(destination.join("settings.json")).unwrap(),
            b"current"
        );
        assert!(
            !fs::symlink_metadata(destination.join("projects/2026-08-08/chat"))
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert!(report.migrated.is_empty());

        fs::remove_dir_all(&root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn sweeps_only_a_dead_migrators_artifacts() {
        let root = test_root("sweep");
        let (legacy, destination) = pair(&root);
        fs::create_dir_all(&legacy).unwrap();
        fs::create_dir_all(&destination).unwrap();
        // A staging directory abandoned by a reaped process, a live one owned
        // by this process, and a file temp from a dead file-copy.
        let mut child = std::process::Command::new("true").spawn().unwrap();
        let dead_pid = child.id();
        child.wait().unwrap();
        let dead = root.join(format!("..goddard.migrating-{dead_pid}"));
        fs::create_dir_all(dead.join("partial")).unwrap();
        let live = root.join(format!("..goddard.migrating-{}", std::process::id()));
        fs::create_dir_all(&live).unwrap();
        fs::write(
            destination.join(format!(".settings.json.migrating-{dead_pid}")),
            b"half",
        )
        .unwrap();
        fs::write(destination.join(".goddard-migration-incomplete"), b"").unwrap();

        sweep_migration_artifacts(&destination);

        assert!(!dead.exists());
        assert!(live.exists());
        assert!(
            !destination
                .join(format!(".settings.json.migrating-{dead_pid}"))
                .exists()
        );
        assert!(!destination.join(".goddard-migration-incomplete").exists());

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn skips_when_there_is_nothing_to_migrate() {
        let root = test_root("none");
        let (legacy, destination) = pair(&root);
        let mut report = MigrationReport::default();
        migrate_home(&legacy, &destination, &mut report);
        assert!(report.migrated.is_empty());
        assert!(report.skipped.is_empty());
        assert!(!destination.exists());
    }
}
