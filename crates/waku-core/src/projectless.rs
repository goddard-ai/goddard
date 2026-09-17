//! Private workspaces for tasks that are not attached to a user project.
//!
//! Codex allocates ordinary projectless chats beneath a per-user root using
//! `<root>/<local date>/<prompt slug>`, with numeric collision suffixes and a
//! random fallback. Goddard mirrors that layout beneath `~/.goddard/projects` so
//! generated workspaces do not sit beside configuration documents.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{OnceLock, RwLock};

use chrono::{Local, NaiveDate};
use uuid::Uuid;

const DEFAULT_SLUG: &str = "new-chat";
const MAX_SLUG_BYTES: usize = 80;
const MAX_NUMBERED_CANDIDATES: usize = 100;
const MAX_RANDOM_CANDIDATES: usize = 5;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Workspace {
    pub cwd: PathBuf,
    pub workspace_root: PathBuf,
}

/// The daemon root is cached because `Project::is_projectless` is reached from
/// row builders and render paths. A remote client installs the daemon-host
/// value during task-state loading; callers then perform path compares only.
fn workspace_root_slot() -> &'static RwLock<Option<PathBuf>> {
    static ROOT: OnceLock<RwLock<Option<PathBuf>>> = OnceLock::new();
    ROOT.get_or_init(|| {
        RwLock::new(dirs::home_dir().map(|home| {
            home.join(waku_protocol::identity::HOME_DIRECTORY_NAME)
                .join("projects")
        }))
    })
}

pub fn set_workspace_root(root: Option<PathBuf>) {
    waku_protocol::projectless::set_workspace_root(root.clone());
    if let Ok(mut current) = workspace_root_slot().write() {
        *current = root;
    }
}

pub fn workspace_root() -> Option<PathBuf> {
    workspace_root_slot().read().ok()?.clone()
}

/// Home directory on the host that owns the configured projectless root.
/// Remote desktops use this only to abbreviate daemon paths for display.
pub fn home_directory() -> Option<PathBuf> {
    let root = workspace_root()?;
    root.parent()?.parent().map(Path::to_path_buf)
}

/// Existing builds created dated workspaces directly under `~/.goddard`; keep
/// recognizing those paths while all new workspaces live under `projects/`.
/// Sessions recorded before the rename may also carry workspaces under the
/// pre-Goddard `~/.waku` home.
pub fn is_projectless_path(path: &Path) -> bool {
    workspace_root().is_some_and(|root| {
        path.starts_with(&root)
            || root
                .parent()
                .is_some_and(|legacy_root| is_legacy_workspace_path(path, legacy_root))
            || legacy_home_root(&root).is_some_and(|legacy_home| {
                path.starts_with(legacy_home.join("projects"))
                    || is_legacy_workspace_path(path, &legacy_home)
            })
    })
}

/// The pre-Goddard home directory beside the workspace root's — `~/.waku`
/// for a local `~/.goddard/projects` root, or the remote install's `.waku`
/// when the client carries a remote root.
fn legacy_home_root(workspace_root: &Path) -> Option<PathBuf> {
    workspace_root
        .parent()?
        .parent()
        .map(|home| home.join(waku_protocol::identity::LEGACY_HOME_DIRECTORY_NAME))
}

/// Whether an existing projectless workspace still uses the pre-`projects/`
/// layout and should be moved by the daemon.
pub fn needs_migration(path: &Path) -> bool {
    let Some(root) = workspace_root() else {
        return false;
    };
    !path.starts_with(&root)
        && root
            .parent()
            .is_some_and(|legacy_root| is_legacy_workspace_path(path, legacy_root))
}

fn is_legacy_workspace_path(path: &Path, legacy_root: &Path) -> bool {
    if path == legacy_root {
        return true;
    }
    let Some(date) = path
        .strip_prefix(legacy_root)
        .ok()
        .and_then(|relative| relative.components().next())
        .and_then(|component| component.as_os_str().to_str())
    else {
        return false;
    };
    is_date_component(date)
}

pub fn is_legacy_root_path(path: &Path) -> bool {
    workspace_root()
        .is_some_and(|root| root.parent().is_some_and(|legacy_root| path == legacy_root))
}

/// `~/.goddard/archives`, beside the projects root. Zipped workspaces keep the
/// dated layout they came from: `<archives>/<date>/<slug>.zip`.
fn archives_root_in(root: &Path) -> Option<PathBuf> {
    root.parent().map(|parent| parent.join("archives"))
}

/// Where a workspace's archive lands. `None` for `~/.goddard` itself, which
/// the oldest layout used as a workspace and now holds configuration.
fn archive_path_in(root: &Path, path: &Path) -> Option<PathBuf> {
    if root.parent().is_some_and(|legacy_root| path == legacy_root) {
        return None;
    }
    let slug = path.file_name()?;
    let archives = archives_root_in(root)?;
    let directory = match path.parent().and_then(|parent| parent.file_name()) {
        Some(date) if is_date_component(date.to_string_lossy().as_ref()) => archives.join(date),
        _ => archives,
    };
    Some(directory.join(format!("{}.zip", slug.to_string_lossy())))
}

/// The archive operations arrive over the wire, so they re-verify the path
/// names a workspace this app owns before touching the filesystem. That
/// bounds a mistaken or hostile request to `~/.goddard`-managed directories.
fn validate_workspace_path_in(root: &Path, path: &Path) -> io::Result<()> {
    let legacy_root = root.parent();
    let legacy_home = legacy_home_root(root);
    let projectless = path.starts_with(root)
        || legacy_root.is_some_and(|legacy_root| is_legacy_workspace_path(path, legacy_root))
        || legacy_home.as_ref().is_some_and(|legacy_home| {
            path != *legacy_home
                && (path.starts_with(legacy_home.join("projects"))
                    || is_legacy_workspace_path(path, legacy_home))
        });
    if projectless && path != root && legacy_root.is_none_or(|legacy_root| path != legacy_root) {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "path is not a projectless workspace",
        ))
    }
}

/// Zip a projectless workspace into `~/.goddard/archives` and remove the live
/// directory. The directory survives a failed capture — the same
/// verify-before-delete rule the worktree cleanup follows.
pub fn archive_workspace(path: &Path) -> io::Result<PathBuf> {
    let root = workspace_root().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "could not locate the home directory for ~/.goddard/projects",
        )
    })?;
    archive_workspace_in(&root, path)
}

fn archive_workspace_in(root: &Path, path: &Path) -> io::Result<PathBuf> {
    validate_workspace_path_in(root, path)?;
    validate_real_directory(path)?;
    let destination = archive_path_in(root, path).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "projectless workspace has no archive destination",
        )
    })?;
    if let Some(parent) = destination.parent() {
        ensure_real_directory(parent)?;
    }
    let staging = destination.with_extension("zip.partial");
    let captured = write_workspace_zip(path, &staging).and_then(|()| {
        // Reopen before the rename: a corrupt archive must not retire the
        // directory it claims to preserve.
        zip::ZipArchive::new(fs::File::open(&staging)?).map_err(zip_io_error)?;
        fs::rename(&staging, &destination)
    });
    if let Err(error) = captured {
        fs::remove_file(&staging).ok();
        return Err(error);
    }
    fs::remove_dir_all(path)?;
    Ok(destination)
}

/// Bring a workspace's archive back to its recorded path. Returns whether
/// the directory exists because of this call; a missing archive still
/// recreates an empty workspace so an unarchived chat lands on a real cwd.
pub fn restore_workspace(path: &Path) -> io::Result<bool> {
    let root = workspace_root().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "could not locate the home directory for ~/.goddard/projects",
        )
    })?;
    restore_workspace_in(&root, path)
}

fn restore_workspace_in(root: &Path, path: &Path) -> io::Result<bool> {
    validate_workspace_path_in(root, path)?;
    if path.exists() {
        return Ok(false);
    }
    let Some(archive) = archive_path_in(root, path) else {
        ensure_real_directory(path)?;
        return Ok(true);
    };
    let file = match fs::File::open(&archive) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            ensure_real_directory(path)?;
            return Ok(true);
        }
        Err(error) => return Err(error),
    };
    // Extract beside the destination first: a partial pull must not leave a
    // half-populated directory that `path.exists()` would later accept.
    let staging = path.with_file_name(format!(
        ".{}-restore-{}",
        path.file_name().unwrap_or_default().to_string_lossy(),
        Uuid::new_v4()
    ));
    let extracted = extract_workspace_zip(file, &staging).and_then(|()| fs::rename(&staging, path));
    if let Err(error) = extracted {
        fs::remove_dir_all(&staging).ok();
        return Err(error);
    }
    // The archive is single-use, same as the worktree snapshot ref.
    fs::remove_file(&archive)?;
    Ok(true)
}

/// Permanently drop a workspace — retention purge and session delete land
/// here once nothing about the task remains. The live directory goes to the
/// Trash like skill installs do; the archive, an app-internal format, is
/// deleted outright.
pub fn remove_workspace(path: &Path) -> io::Result<()> {
    let root = workspace_root().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "could not locate the home directory for ~/.goddard/projects",
        )
    })?;
    remove_workspace_in(&root, path)
}

fn remove_workspace_in(root: &Path, path: &Path) -> io::Result<()> {
    validate_workspace_path_in(root, path)?;
    if let Some(archive) = archive_path_in(root, path) {
        match fs::remove_file(&archive) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    match validate_real_directory(path) {
        Ok(()) => trash::delete(path).map_err(|error| {
            io::Error::new(io::ErrorKind::Other, format!("could not trash workspace: {error}"))
        }),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn zip_io_error(error: zip::result::ZipError) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error)
}

/// The zip stores workspace-relative paths so extraction can land wherever
/// the session's recorded path points. Symlinks are preserved as symlinks —
/// agent workspaces carry them in dependency trees like `node_modules/.bin`.
fn write_workspace_zip(root: &Path, destination: &Path) -> io::Result<()> {
    let mut writer = zip::ZipWriter::new(fs::File::create(destination)?);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    let mut pending = vec![PathBuf::new()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(root.join(&directory))? {
            let entry = entry?;
            let relative = directory.join(entry.file_name());
            let name = relative.to_string_lossy().into_owned();
            let file_type = entry.file_type()?;
            if file_type.is_dir() {
                writer
                    .add_directory(name, options)
                    .map_err(zip_io_error)?;
                pending.push(relative);
            } else if file_type.is_symlink() {
                let target = fs::read_link(entry.path())?;
                writer
                    .add_symlink(name, target.to_string_lossy().into_owned(), options)
                    .map_err(zip_io_error)?;
            } else if file_type.is_file() {
                writer.start_file(name, options).map_err(zip_io_error)?;
                io::copy(&mut fs::File::open(entry.path())?, &mut writer)?;
            }
        }
    }
    writer.finish().map_err(zip_io_error)?;
    Ok(())
}

fn extract_workspace_zip(archive: fs::File, destination: &Path) -> io::Result<()> {
    let mut archive = zip::ZipArchive::new(archive).map_err(zip_io_error)?;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(zip_io_error)?;
        // `enclosed_name` refuses absolute paths and `..` escapes.
        let Some(relative) = entry.enclosed_name() else {
            continue;
        };
        let target = destination.join(relative);
        #[cfg(unix)]
        let is_symlink = entry
            .unix_mode()
            .is_some_and(|mode| mode & 0o170000 == 0o120000);
        #[cfg(not(unix))]
        let is_symlink = false;
        if is_symlink {
            #[cfg(unix)]
            {
                if let Some(parent) = target.parent() {
                    fs::create_dir_all(parent)?;
                }
                let mut link_target = String::new();
                io::Read::read_to_string(&mut entry, &mut link_target)?;
                std::os::unix::fs::symlink(link_target, &target)?;
            }
        } else if entry.is_dir() {
            fs::create_dir_all(&target)?;
        } else {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            io::copy(&mut entry, &mut fs::File::create(&target)?)?;
        }
    }
    Ok(())
}

pub fn create_workspace(prompt: Option<&str>) -> io::Result<Workspace> {
    let root = workspace_root().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "could not locate the home directory for ~/.goddard/projects",
        )
    })?;
    create_workspace_in(&root, Local::now().date_naive(), None, prompt)
}

/// Move one old dated workspace from `~/.goddard/<date>/<slug>` into
/// `~/.goddard/projects/<date>/<slug>` without copying its contents through the
/// client. The oldest layout used `~/.goddard` itself; that path contains Goddard's
/// configuration now, so it receives a fresh private workspace instead of
/// moving the configuration directory.
pub fn migrate_workspace(path: &Path) -> io::Result<Workspace> {
    let root = workspace_root().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "could not locate the home directory for ~/.goddard/projects",
        )
    })?;
    migrate_workspace_in(&root, path)
}

fn migrate_workspace_in(root: &Path, path: &Path) -> io::Result<Workspace> {
    if path.starts_with(root) {
        validate_real_directory(path)?;
        return Ok(Workspace {
            cwd: path.to_owned(),
            workspace_root: root.to_owned(),
        });
    }
    let legacy_root = root.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid projectless workspace root",
        )
    })?;
    if path == legacy_root {
        return create_workspace_in(root, Local::now().date_naive(), None, None);
    }

    let relative = path.strip_prefix(legacy_root).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "path is not a legacy projectless workspace",
        )
    })?;
    let components = relative.components().collect::<Vec<_>>();
    if components.len() != 2
        || components
            .iter()
            .any(|component| !matches!(component, std::path::Component::Normal(_)))
        || !is_date_component(components[0].as_os_str().to_string_lossy().as_ref())
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "path is not a dated legacy projectless workspace",
        ));
    }
    validate_real_directory(path)?;
    ensure_real_directory(root)?;
    let date_directory = root.join(components[0].as_os_str());
    ensure_real_directory(&date_directory)?;
    let original_name = components[1].as_os_str().to_string_lossy();
    for index in 0..MAX_NUMBERED_CANDIDATES {
        let name = if index == 0 {
            original_name.to_string()
        } else {
            format!("{original_name}-{}", index + 1)
        };
        let destination = date_directory.join(name);
        if destination.exists() {
            continue;
        }
        match fs::rename(path, &destination) {
            Ok(()) => {
                return Ok(Workspace {
                    cwd: destination,
                    workspace_root: root.to_owned(),
                });
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "unable to allocate a destination for the legacy projectless workspace",
    ))
}

fn is_date_component(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| matches!(index, 4 | 7) || byte.is_ascii_digit())
}

fn workspace_slug(directory_name: Option<&str>, prompt: Option<&str>) -> String {
    let directory_name = directory_name.filter(|value| !value.trim().is_empty());
    let prompt = prompt.filter(|value| !value.trim().is_empty());
    let source = directory_name.or(prompt).unwrap_or_default().to_lowercase();
    let mut words = source
        .split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|word| !word.is_empty());
    let maximum_words = if directory_name.is_some() {
        usize::MAX
    } else {
        6
    };
    let mut slug = words
        .by_ref()
        .take(maximum_words)
        .collect::<Vec<_>>()
        .join("-");
    slug.truncate(MAX_SLUG_BYTES);
    if slug.is_empty() {
        DEFAULT_SLUG.to_owned()
    } else {
        slug
    }
}

fn create_workspace_in(
    root: &Path,
    date: NaiveDate,
    directory_name: Option<&str>,
    prompt: Option<&str>,
) -> io::Result<Workspace> {
    ensure_real_directory(root)?;
    let date_directory = root.join(date.format("%Y-%m-%d").to_string());
    ensure_real_directory(&date_directory)?;
    let slug = workspace_slug(directory_name, prompt);

    for index in 0..MAX_NUMBERED_CANDIDATES {
        let name = if index == 0 {
            slug.clone()
        } else {
            format!("{slug}-{}", index + 1)
        };
        if let Some(workspace) = try_create_workspace(root, &date_directory, &name)? {
            return Ok(workspace);
        }
    }

    for _ in 0..MAX_RANDOM_CANDIDATES {
        let name = format!("{slug}-{}", Uuid::new_v4());
        if let Some(workspace) = try_create_workspace(root, &date_directory, &name)? {
            return Ok(workspace);
        }
    }

    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "unable to create a unique projectless task directory",
    ))
}

fn try_create_workspace(
    root: &Path,
    date_directory: &Path,
    name: &str,
) -> io::Result<Option<Workspace>> {
    let cwd = date_directory.join(name);
    match fs::create_dir(&cwd) {
        Ok(()) => Ok(Some(Workspace {
            cwd,
            workspace_root: root.to_owned(),
        })),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            validate_real_directory(&cwd)?;
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

fn ensure_real_directory(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(_) => validate_real_directory(path),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            fs::create_dir_all(path)?;
            validate_real_directory(path)
        }
        Err(error) => Err(error),
    }
}

fn validate_real_directory(path: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.is_dir() && !metadata.file_type().is_symlink() {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "projectless task directory must be a real directory",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_root() -> PathBuf {
        std::env::temp_dir().join(format!("waku-projectless-{}", Uuid::new_v4()))
    }

    #[test]
    fn prompt_slug_matches_codex_word_and_length_rules() {
        assert_eq!(
            workspace_slug(
                None,
                Some("Build A polished, LOCAL coding agent please now")
            ),
            "build-a-polished-local-coding-agent"
        );
        assert_eq!(workspace_slug(None, Some("你好 👋")), "new-chat");
        assert_eq!(
            workspace_slug(Some("Release Candidate Number 12"), Some("ignored prompt")),
            "release-candidate-number-12"
        );
        assert_eq!(
            workspace_slug(Some("  "), Some("Prompt fallback")),
            "prompt-fallback"
        );
        assert_eq!(
            workspace_slug(Some(&"a".repeat(100)), None).len(),
            MAX_SLUG_BYTES
        );
    }

    #[test]
    fn creates_date_and_unique_task_directories_without_split_folders() {
        let root = test_root();
        let date = NaiveDate::from_ymd_opt(2026, 8, 8).unwrap();

        let first = create_workspace_in(&root, date, None, Some("Fix projectless sessions"))
            .expect("first workspace");
        let second = create_workspace_in(&root, date, None, Some("Fix projectless sessions"))
            .expect("second workspace");

        assert_eq!(first.cwd, root.join("2026-08-08/fix-projectless-sessions"));
        assert_eq!(
            second.cwd,
            root.join("2026-08-08/fix-projectless-sessions-2")
        );
        assert_eq!(first.workspace_root, root);
        assert_eq!(fs::read_dir(&first.cwd).unwrap().count(), 0);

        fs::remove_dir_all(&root).ok();
    }

    #[cfg(unix)]
    #[test]
    fn refuses_a_symlinked_workspace_root() {
        use std::os::unix::fs::symlink;

        let parent = test_root();
        let target = parent.join("target");
        let root = parent.join("root");
        fs::create_dir_all(&target).unwrap();
        symlink(&target, &root).unwrap();

        let error = create_workspace_in(
            &root,
            NaiveDate::from_ymd_opt(2026, 8, 8).unwrap(),
            None,
            None,
        )
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);

        fs::remove_dir_all(&parent).ok();
    }

    #[test]
    fn migrates_legacy_workspace_with_contents_under_projects() {
        let legacy_root = test_root();
        let root = legacy_root.join("projects");
        let legacy = legacy_root.join("2026-08-08/fix-projectless-sessions");
        fs::create_dir_all(&legacy).unwrap();
        fs::write(legacy.join("notes.txt"), "kept").unwrap();

        let migrated = migrate_workspace_in(&root, &legacy).unwrap();

        assert_eq!(
            migrated.cwd,
            root.join("2026-08-08/fix-projectless-sessions")
        );
        assert_eq!(
            fs::read_to_string(migrated.cwd.join("notes.txt")).unwrap(),
            "kept"
        );
        assert!(!legacy.exists());
        fs::remove_dir_all(&legacy_root).ok();
    }

    #[test]
    fn archive_round_trips_a_workspace_through_a_dated_zip() {
        let home = test_root();
        let root = home.join("projects");
        let workspace = root.join("2026-09-16/fix-the-bug");
        fs::create_dir_all(workspace.join("src")).unwrap();
        fs::write(workspace.join("notes.txt"), "kept\n").unwrap();
        fs::write(workspace.join("src/main.rs"), "fn main() {}\n").unwrap();

        let archive = archive_workspace_in(&root, &workspace).unwrap();

        assert_eq!(archive, home.join("archives/2026-09-16/fix-the-bug.zip"));
        assert!(!workspace.exists());

        assert!(restore_workspace_in(&root, &workspace).unwrap());
        assert_eq!(
            fs::read_to_string(workspace.join("notes.txt")).unwrap(),
            "kept\n"
        );
        assert_eq!(
            fs::read_to_string(workspace.join("src/main.rs")).unwrap(),
            "fn main() {}\n"
        );
        // The archive is single-use: a consumed zip does not shadow a
        // workspace that churns again.
        assert!(!archive.exists());
        // Restoring an existing directory is a no-op.
        assert!(!restore_workspace_in(&root, &workspace).unwrap());

        fs::remove_dir_all(&home).ok();
    }

    #[test]
    fn restore_recreates_an_empty_workspace_without_an_archive() {
        let home = test_root();
        let root = home.join("projects");
        let workspace = root.join("2026-09-16/never-archived");

        assert!(restore_workspace_in(&root, &workspace).unwrap());
        assert!(workspace.is_dir());

        fs::remove_dir_all(&home).ok();
    }

    #[test]
    fn workspace_operations_refuse_paths_outside_the_projects_root() {
        let home = test_root();
        let root = home.join("projects");
        let outside = home.join("elsewhere");
        fs::create_dir_all(&outside).unwrap();

        assert!(archive_workspace_in(&root, &outside).is_err());
        assert!(archive_workspace_in(&root, &root).is_err());
        // `~/.goddard` itself was the oldest layout's workspace; it holds
        // configuration now and must never be archived or removed.
        assert!(archive_workspace_in(&root, &home).is_err());
        assert!(restore_workspace_in(&root, &outside).is_err());
        assert!(remove_workspace_in(&root, &outside).is_err());
        assert!(remove_workspace_in(&root, &home).is_err());
        assert!(outside.exists());

        fs::remove_dir_all(&home).ok();
    }

    #[test]
    fn remove_workspace_drops_the_archive_file() {
        let home = test_root();
        let root = home.join("projects");
        let workspace = root.join("2026-09-16/fix-the-bug");
        fs::create_dir_all(&workspace).unwrap();
        let archive = archive_workspace_in(&root, &workspace).unwrap();
        assert!(archive.exists());

        remove_workspace_in(&root, &workspace).unwrap();

        assert!(!archive.exists());
        fs::remove_dir_all(&home).ok();
    }

    #[test]
    fn legacy_dated_workspace_archives_under_the_same_date() {
        let home = test_root();
        let root = home.join("projects");
        let legacy = home.join("2026-08-08/old-chat");
        fs::create_dir_all(&legacy).unwrap();
        fs::write(legacy.join("a.txt"), "x").unwrap();

        let archive = archive_workspace_in(&root, &legacy).unwrap();

        assert_eq!(archive, home.join("archives/2026-08-08/old-chat.zip"));
        assert!(!legacy.exists());
        fs::remove_dir_all(&home).ok();
    }

    #[cfg(unix)]
    #[test]
    fn archive_and_restore_preserve_symlinks() {
        let home = test_root();
        let root = home.join("projects");
        let workspace = root.join("2026-09-16/links");
        fs::create_dir_all(&workspace).unwrap();
        fs::write(workspace.join("real.txt"), "body").unwrap();
        std::os::unix::fs::symlink("real.txt", workspace.join("link.txt")).unwrap();

        archive_workspace_in(&root, &workspace).unwrap();
        assert!(restore_workspace_in(&root, &workspace).unwrap());

        assert_eq!(
            fs::read_link(workspace.join("link.txt")).unwrap(),
            Path::new("real.txt")
        );
        fs::remove_dir_all(&home).ok();
    }
}
