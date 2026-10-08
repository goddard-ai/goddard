//! Stable daemon-owned copies of packaged runtime resources.
use anyhow::{Context as _, anyhow};
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// Locate the `goddard-agent` binary to place on a provider's `PATH`.
/// Packaged macOS builds prefer their own Resources copy, retaining a
/// commit-scoped fallback for a bundle lost under a running daemon.
pub fn agent_cli_path() -> anyhow::Result<PathBuf> {
    let executable =
        std::env::current_exe().context("Goddard daemon executable path is unavailable")?;
    let name = if cfg!(windows) {
        "goddard-agent.exe"
    } else {
        "goddard-agent"
    };
    let bundled = executable
        .parent()
        .and_then(|macos| macos.parent())
        .map(|contents| contents.join("Resources").join(name));
    #[cfg(target_os = "macos")]
    let candidates = [bundled, Some(executable.with_file_name(name))];
    #[cfg(not(target_os = "macos"))]
    let candidates = [Some(executable.with_file_name(name)), bundled];
    let packaged = candidates.into_iter().flatten().find(|path| path.is_file());
    #[cfg(all(target_os = "macos", debug_assertions))]
    {
        let commit = option_env!("GODDARD_COMMIT_SHA").context(
            "this debug daemon has no build identity; rebuild before using its agent CLI",
        )?;
        let staged = runtime_install_root()?.join(commit).join(name);
        return resolve_debug_cli(packaged.as_deref(), &staged, commit);
    }
    #[cfg(not(all(target_os = "macos", debug_assertions)))]
    staged_resource(packaged.as_deref(), name, None)
        .ok_or_else(|| anyhow!("the goddard-agent CLI is missing from this Goddard build"))
}

#[cfg(all(target_os = "macos", debug_assertions))]
fn resolve_debug_cli(
    packaged: Option<&Path>,
    staged: &Path,
    commit: &str,
) -> anyhow::Result<PathBuf> {
    if let Some(source) = packaged {
        verify_cli_commit(source, commit)?;
        if let Err(error) = refresh_staged(source, staged, None) {
            eprintln!(
                "goddard-daemon: could not stage {}: {error:#}",
                source.display()
            );
        }
        return Ok(source.to_path_buf());
    }
    if staged.is_file() {
        verify_cli_commit(staged, commit)?;
        return Ok(staged.to_path_buf());
    }
    anyhow::bail!(
        "the goddard-agent CLI for daemon build {commit} is missing from its bundle and {}; rebuild the debug runtime (b + enter in the watcher)",
        staged.display()
    )
}

#[cfg(all(target_os = "macos", debug_assertions))]
fn verify_cli_commit(path: &Path, expected: &str) -> anyhow::Result<()> {
    let output = std::process::Command::new(path)
        .arg("--build-commit")
        .output()
        .with_context(|| {
            format!(
                "could not read agent CLI build identity from {}",
                path.display()
            )
        })?;
    let actual = String::from_utf8_lossy(&output.stdout);
    let actual = actual.trim();
    if !output.status.success() || actual != expected {
        anyhow::bail!(
            "refusing stale goddard-agent at {}: CLI build {}, daemon build {expected}; rebuild the debug runtime (b + enter in the watcher)",
            path.display(),
            if actual.is_empty() { "unknown" } else { actual }
        );
    }
    Ok(())
}

/// A daemon-owned copy of every resource the agent surface and Computer Use
/// resolve beside the executable. The packaged directory can vanish under a
/// running daemon — a build-cache garbage collection, a rebuilt target
/// directory, a swapped app bundle — and each executable-relative lookup
/// then fails at once. Resolvers refresh their staged copy whenever the
/// packaged source exists and fall back to the last copy when it is gone.
pub fn runtime_install_root() -> anyhow::Result<PathBuf> {
    Ok(dirs::data_local_dir()
        .or_else(dirs::data_dir)
        .ok_or_else(|| anyhow!("application support directory is unavailable"))?
        .join(waku_base::identity::DATA_DIRECTORY_NAME)
        .join("Runtime"))
}

/// Resolve a packaged runtime resource to a stable path. With `packaged`
/// present, the staged copy is refreshed and returned (the source itself on
/// a failed refresh); without it, the last staged copy answers. `key` names
/// a small file whose bytes version a staged directory — a helper bundle's
/// build fingerprint, a skills tree's entry document.
pub fn staged_resource(packaged: Option<&Path>, name: &str, key: Option<&Path>) -> Option<PathBuf> {
    let staged = runtime_install_root().ok().map(|root| root.join(name));
    resolve_staged(packaged, staged.as_deref(), key)
}

fn resolve_staged(
    packaged: Option<&Path>,
    staged: Option<&Path>,
    key: Option<&Path>,
) -> Option<PathBuf> {
    match (packaged, staged) {
        (Some(source), Some(staged)) => match refresh_staged(source, staged, key) {
            Ok(()) => Some(staged.to_path_buf()),
            Err(error) => {
                eprintln!(
                    "goddard-daemon: could not stage {}: {error:#}",
                    source.display()
                );
                Some(source.to_path_buf())
            }
        },
        (Some(source), None) => Some(source.to_path_buf()),
        (None, Some(staged)) if staged.exists() => Some(staged.to_path_buf()),
        (None, _) => None,
    }
}

/// Copy `source` over `staged` when they differ, through a sibling staging
/// name so a concurrent reader never sees a partial result. Files compare
/// on size and mtime (the copy is stamped back to the source's); directories
/// compare on `key`'s bytes.
fn refresh_staged(source: &Path, staged: &Path, key: Option<&Path>) -> anyhow::Result<()> {
    if source.is_dir() {
        let key = key.context("a staged directory needs a key file to compare")?;
        if fs::read(staged.join(key))
            .is_ok_and(|staged| Some(staged) == fs::read(source.join(key)).ok())
        {
            return Ok(());
        }
    } else {
        let unchanged = fs::metadata(source)
            .ok()
            .zip(fs::metadata(staged).ok())
            .is_some_and(|(source, staged)| {
                source.len() == staged.len() && source.modified().ok() == staged.modified().ok()
            });
        if unchanged {
            return Ok(());
        }
    }
    waku_base::fs_ext::create_private_dir_all(staged.parent().unwrap_or(Path::new(".")))?;
    let staging = staged.with_file_name(format!(".stage-{}", Uuid::new_v4().simple()));
    let result = if source.is_dir() {
        copy_directory(source, &staging)
    } else {
        fs::copy(source, &staging)
            .map(|_| ())
            .and_then(|()| {
                fs::File::open(&staging)?.set_modified(fs::metadata(source)?.modified()?)
            })
            .map_err(anyhow::Error::from)
    };
    if let Err(error) = result {
        let _ = fs::remove_dir_all(&staging).or_else(|_| fs::remove_file(&staging));
        return Err(error);
    }
    if staged.is_dir() {
        fs::remove_dir_all(staged)?;
    } else if staged.exists() {
        fs::remove_file(staged)?;
    }
    fs::rename(&staging, staged)?;
    Ok(())
}

#[doc(hidden)]
pub fn copy_directory(source: &Path, destination: &Path) -> anyhow::Result<()> {
    let metadata = fs::symlink_metadata(source)?;
    fs::create_dir(destination)?;
    fs::set_permissions(destination, metadata.permissions())?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            copy_directory(&source_path, &destination_path)?;
        } else if file_type.is_symlink() {
            waku_base::fs_ext::symlink(&fs::read_link(&source_path)?, &destination_path)?;
        } else {
            fs::copy(&source_path, &destination_path)?;
            fs::set_permissions(
                &destination_path,
                fs::symlink_metadata(&source_path)?.permissions(),
            )?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    const HELPER_FINGERPRINT_PATH: &str = "Contents/Resources/.goddard-helper-fingerprint";
    #[cfg(all(target_os = "macos", debug_assertions))]
    #[test]
    fn debug_cli_prefers_its_bundle_and_refuses_a_stale_fallback() {
        use std::os::unix::fs::PermissionsExt as _;
        let root = std::env::temp_dir().join(format!("runtime-identity-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let packaged = root.join("goddard-agent");
        let staged = root.join("Runtime").join("commit-a").join("goddard-agent");
        let cli = |path: &Path, commit: &str| {
            fs::write(path, format!("#!/bin/sh\nprintf '%s\\n' '{commit}'\n")).unwrap();
            fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
        };
        cli(&packaged, "commit-a");
        assert_eq!(
            resolve_debug_cli(Some(&packaged), &staged, "commit-a").unwrap(),
            packaged
        );
        assert!(staged.is_file());
        fs::remove_file(&packaged).unwrap();
        assert_eq!(
            resolve_debug_cli(None, &staged, "commit-a").unwrap(),
            staged
        );
        cli(&staged, "commit-b");
        let error = resolve_debug_cli(None, &staged, "commit-a")
            .unwrap_err()
            .to_string();
        assert!(error.contains("CLI build commit-b, daemon build commit-a"));
        assert!(error.contains("b + enter"));
        // The bundle wins even if an existing staged copy has the wrong stamp.
        cli(&packaged, "commit-a");
        assert_eq!(
            resolve_debug_cli(Some(&packaged), &staged, "commit-a").unwrap(),
            packaged
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn staged_resources_survive_a_lost_packaged_copy() {
        let root = std::env::temp_dir().join(format!("runtime-stage-{}", Uuid::new_v4()));
        let packaged_dir = root.join("packaged");
        fs::create_dir_all(&packaged_dir).unwrap();
        let packaged = packaged_dir.join("goddard-agent");
        fs::write(&packaged, b"v1").unwrap();
        let staged = root.join("runtime").join("goddard-agent");

        // The packaged copy stages on first resolve...
        assert_eq!(
            resolve_staged(Some(&packaged), Some(&staged), None).unwrap(),
            staged
        );
        assert_eq!(fs::read(&staged).unwrap(), b"v1");

        // ...and keeps answering after the packaged copy is gone.
        fs::remove_dir_all(&packaged_dir).unwrap();
        assert_eq!(resolve_staged(None, Some(&staged), None).unwrap(), staged);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn staged_resources_refresh_when_the_packaged_copy_changes() {
        let root = std::env::temp_dir().join(format!("runtime-stage-{}", Uuid::new_v4()));
        let packaged_dir = root.join("packaged");
        fs::create_dir_all(&packaged_dir).unwrap();
        let packaged = packaged_dir.join("goddard-agent");
        fs::write(&packaged, b"v1").unwrap();
        let staged = root.join("runtime").join("goddard-agent");

        resolve_staged(Some(&packaged), Some(&staged), None).unwrap();
        // A changed size forces a refresh without depending on mtime
        // granularity.
        fs::write(&packaged, b"v2-rebuilt").unwrap();
        assert_eq!(
            resolve_staged(Some(&packaged), Some(&staged), None).unwrap(),
            staged
        );
        assert_eq!(fs::read(&staged).unwrap(), b"v2-rebuilt");

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn staged_directories_compare_on_their_key_file() {
        let root = std::env::temp_dir().join(format!("runtime-stage-{}", Uuid::new_v4()));
        let packaged = bundled_helper(&root, "aaaa1111");
        let staged = root.join("runtime").join("Goddard Computer Use.app");
        let key = Path::new(HELPER_FINGERPRINT_PATH);

        assert_eq!(
            resolve_staged(Some(&packaged), Some(&staged), Some(key)).unwrap(),
            staged
        );
        assert_eq!(
            fs::read_to_string(staged.join(key)).unwrap().trim(),
            "aaaa1111"
        );

        // A rebuilt bundle stages over the old copy; a lost one falls back
        // to what is already staged.
        let rebuilt = bundled_helper(&root, "bbbb2222");
        fs::remove_dir_all(packaged.parent().unwrap()).unwrap();
        fs::rename(rebuilt.parent().unwrap(), packaged.parent().unwrap()).unwrap();
        resolve_staged(Some(&packaged), Some(&staged), Some(key)).unwrap();
        assert_eq!(
            fs::read_to_string(staged.join(key)).unwrap().trim(),
            "bbbb2222"
        );
        fs::remove_dir_all(packaged.parent().unwrap()).unwrap();
        assert_eq!(
            resolve_staged(None, Some(&staged), Some(key)).unwrap(),
            staged
        );

        let _ = fs::remove_dir_all(&root);
    }

    fn bundled_helper(root: &Path, fingerprint: &str) -> PathBuf {
        let source = root
            .join(format!("bundled-{fingerprint}"))
            .join("Goddard Computer Use.app");
        fs::create_dir_all(source.join("Contents/MacOS")).unwrap();
        fs::create_dir_all(source.join("Contents/Resources")).unwrap();
        fs::write(
            source.join(HELPER_FINGERPRINT_PATH),
            format!("{fingerprint}\n"),
        )
        .unwrap();
        fs::write(
            source.join("Contents/MacOS/Goddard Computer Use"),
            b"helper",
        )
        .unwrap();
        source
    }
}
