//! Headless Computer Use state and helper lifecycle.

use std::fs;
#[cfg(target_os = "macos")]
use std::io::Write as _;
use std::path::{Path, PathBuf};
#[cfg(target_os = "macos")]
use std::process::Stdio;
#[cfg(target_os = "macos")]
use std::sync::atomic::{AtomicU32, Ordering};

use anyhow::{Context as _, anyhow, bail};
use base64::Engine as _;
use serde::Deserialize;
use serde_json::Value;
#[cfg(target_os = "macos")]
use serde_json::json;
use uuid::Uuid;

#[cfg(target_os = "macos")]
const MAX_HELPER_OUTPUT_BYTES: usize = 24 * 1024 * 1024;

pub use waku_protocol::computer_use::{
    ComputerAppGrant, ComputerPermissions, ComputerTarget, ComputerUsePhase, ComputerUseState,
    resolve_enabled,
};

#[derive(Clone, Debug)]
pub struct ComputerToolRequest {
    pub call_id: String,
    pub tool: String,
    pub arguments: Value,
}

impl ComputerToolRequest {
    pub fn summary(&self) -> String {
        if self.tool != "use" {
            return match self.tool.as_str() {
                "status" => "Check computer-use access".into(),
                _ => self.tool.clone(),
            };
        }
        let actions = self
            .arguments
            .get("actions")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default();
        if actions.is_empty() {
            return "Inspect the window".into();
        }
        let mut labels = actions
            .iter()
            .filter_map(|action| action.get("type").and_then(Value::as_str))
            .map(action_label)
            .collect::<Vec<_>>();
        labels.dedup();
        format!("{} {}", labels.join(", "), plural(actions.len(), "action"))
    }
}

fn action_label(action: &str) -> &'static str {
    match action {
        "click" | "double_click" => "Click",
        "move" => "Move the pointer",
        "drag" => "Drag",
        "scroll" => "Scroll",
        "type" => "Type text",
        "keypress" => "Press keys",
        "wait" => "Wait",
        _ => "Interact",
    }
}

fn plural(count: usize, noun: &str) -> String {
    if count == 1 {
        format!("1 {noun}")
    } else {
        format!("{count} {noun}s")
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ComputerUsePreviewUpdate {
    target: ComputerTarget,
    image_url: String,
}

pub fn decode_preview_update(data: &[u8]) -> anyhow::Result<ComputerUseState> {
    let update: ComputerUsePreviewUpdate =
        serde_json::from_slice(data).context("Computer Use preview is invalid JSON")?;
    validate_preview_image_url(&update.image_url)?;
    Ok(ComputerUseState {
        target: Some(update.target),
        phase: ComputerUsePhase::Running,
        visible: true,
        image_url: Some(update.image_url),
    })
}

fn validate_preview_image_url(image_url: &str) -> anyhow::Result<()> {
    const PNG_PREFIX: &str = "data:image/png;base64,";
    let encoded = image_url
        .strip_prefix(PNG_PREFIX)
        .ok_or_else(|| anyhow!("Computer Use preview is not a PNG data URL"))?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .context("Computer Use preview contains invalid base64")?;
    if bytes.is_empty() {
        bail!("Computer Use preview is empty");
    }
    Ok(())
}

#[derive(Clone, Debug)]
pub struct PendingComputerApproval {
    pub request: ComputerToolRequest,
    pub target: ComputerTarget,
    pub sensitive: bool,
}

#[cfg(target_os = "macos")]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HelperResponse {
    success: bool,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    permissions: Option<ComputerPermissions>,
}

#[cfg(target_os = "macos")]
pub fn probe_permissions(prompt: bool) -> anyhow::Result<ComputerPermissions> {
    let operation = if prompt {
        json!({"operation": "requestPermissions"})
    } else {
        json!({"operation": "status"})
    };
    let helper = mcp_server_command()?;
    let active_helper_pid = AtomicU32::new(0);
    let response = invoke_helper_direct(&helper, &operation, &active_helper_pid)?;
    if !response.success {
        bail!(
            "{}",
            response
                .error
                .unwrap_or_else(|| tr!("computer_use.permission_check_failed"))
        );
    }
    Ok(response.permissions.unwrap_or_default())
}

#[cfg(not(target_os = "macos"))]
pub fn probe_permissions(_prompt: bool) -> anyhow::Result<ComputerPermissions> {
    bail!(
        "Use Cua Driver check_permissions to inspect this desktop's capture and input capabilities"
    )
}

#[cfg(target_os = "macos")]
fn invoke_helper_direct(
    helper: &Path,
    operation: &Value,
    active_helper_pid: &AtomicU32,
) -> anyhow::Result<HelperResponse> {
    let mode = match operation.get("operation").and_then(Value::as_str) {
        Some("status") => Some("status"),
        Some("requestPermissions") => Some("request-permissions"),
        _ => None,
    };
    let mut command = crate::command_env::plain_command(helper);
    if let Some(mode) = mode {
        command.arg(mode);
    }
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command
        .spawn_bounded()
        .with_context(|| format!("failed to start {}", helper.display()))?;
    let pid = child.id();
    active_helper_pid.store(pid, Ordering::SeqCst);
    let payload = serde_json::to_vec(operation)?;
    child
        .stdin()
        .ok_or_else(|| anyhow!("computer-use helper stdin unavailable"))?
        .write_all(&payload)?;
    let output = child.wait_with_output()?;
    let _ = active_helper_pid.compare_exchange(pid, 0, Ordering::SeqCst, Ordering::SeqCst);
    if output.stdout.len() > MAX_HELPER_OUTPUT_BYTES {
        bail!("computer-use helper returned too much data");
    }
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("computer-use helper failed: {}", stderr.trim());
    }
    serde_json::from_slice(&output.stdout).context("computer-use helper returned invalid JSON")
}

fn helper_app_path() -> anyhow::Result<PathBuf> {
    let executable = host_executable_path()?;
    let macos = executable
        .parent()
        .ok_or_else(|| anyhow!("Goddard executable has no parent directory"))?;
    let contents = macos
        .parent()
        .ok_or_else(|| anyhow!("Goddard app bundle is malformed"))?;
    let app_name = executable
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| anyhow!("Goddard executable name is invalid"))?;
    let helper_name = format!("{app_name} Computer Use");
    let name = format!("{helper_name}.app");
    let path = contents.join("Helpers").join(&name);
    let packaged = path.is_dir().then_some(path.as_path());
    staged_resource(packaged, &name, Some(Path::new(HELPER_FINGERPRINT_PATH)))
        .ok_or_else(|| anyhow!("Computer Use helper is missing from this Goddard build"))
}

pub fn helper_display_name() -> String {
    host_executable_path()
        .ok()
        .and_then(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
        .map(|app_name| format!("{app_name} Computer Use"))
        .unwrap_or_else(|| "Goddard Computer Use".into())
}

pub fn mcp_server_command() -> anyhow::Result<PathBuf> {
    if !cfg!(target_os = "macos") {
        let executable = host_executable_path()?;
        let name = helper_executable_name();
        let path = executable.with_file_name(name);
        let packaged = path.is_file().then_some(path.as_path());
        let helper = staged_resource(packaged, name, None)
            .ok_or_else(|| anyhow!("Cua Driver helper is missing from this Goddard build"))?;
        // The helper loads its SDK library from its own directory — stage
        // the siblings it needs beside the staged copy.
        if let Some(directory) = packaged.and_then(Path::parent) {
            for sibling in helper_sibling_names() {
                let source = directory.join(sibling);
                staged_resource(source.is_file().then_some(source.as_path()), sibling, None);
            }
        }
        return Ok(helper);
    }
    let bundled_helper = helper_app_path()?;
    let helper = install_helper_app(&bundled_helper)?;
    let executable = helper
        .file_stem()
        .ok_or_else(|| anyhow!("Computer Use helper name is invalid"))?;
    Ok(helper.join("Contents").join("MacOS").join(executable))
}

fn helper_executable_name() -> &'static str {
    if cfg!(windows) {
        "goddard_computer_use.exe"
    } else {
        "goddard_computer_use"
    }
}

/// Files `goddard_computer_use` loads from its own directory — kept in sync
/// with `library_name()` in crates/waku-computer-use and the packaging list
/// in scripts/cua-driver.ts. Only consulted by the flat-layout branch:
/// macOS carries the SDK inside the helper bundle instead.
fn helper_sibling_names() -> &'static [&'static str] {
    if cfg!(windows) {
        &["cua_driver_sdk.dll", "cua-driver-uia.exe"]
    } else {
        &["libcua_driver_sdk.so"]
    }
}

fn resources_directory(executable: &Path, os: &str) -> anyhow::Result<PathBuf> {
    let directory = executable
        .parent()
        .ok_or_else(|| anyhow!("Goddard executable has no parent"))?;
    Ok(match os {
        "macos" => directory
            .parent()
            .ok_or_else(|| anyhow!("Goddard app bundle is malformed"))?
            .join("Resources"),
        "linux" if directory.file_name().is_some_and(|name| name == "bin") => directory
            .parent()
            .ok_or_else(|| anyhow!("Goddard installation is malformed"))?
            .join("share/goddard"),
        _ => directory.join("resources"),
    })
}

fn packaged_file(path: &Path, name: &str) -> anyhow::Result<PathBuf> {
    if !path.is_file() {
        bail!(
            "{name} is missing from this Goddard build: {}",
            path.display()
        );
    }
    Ok(path.to_path_buf())
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
        .join(crate::identity::DATA_DIRECTORY_NAME)
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
    crate::fs_ext::create_private_dir_all(staged.parent().unwrap_or(Path::new(".")))?;
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

/// Refresh every staged runtime resource while the packaged copies still
/// exist. Runs once at daemon startup so a later loss of the executable's
/// directory leaves working fallbacks; resolvers also stage lazily on each
/// call, so this only narrows the unprotected window after boot.
pub fn stage_runtime_resources() {
    let _ = crate::agent::agent_cli_path();
    let _ = js_repl_server_path();
    let _ = mcp_server_command();
    let _ = skill_root_path();
}

pub fn js_repl_server_path() -> anyhow::Result<PathBuf> {
    let executable = host_executable_path()?;
    let name = if cfg!(windows) {
        "goddard_js_repl.exe"
    } else {
        "goddard_js_repl"
    };
    let path = if cfg!(target_os = "macos") {
        resources_directory(&executable, "macos")?.join(name)
    } else {
        executable.with_file_name(name)
    };
    let packaged = path.is_file().then_some(path.as_path());
    staged_resource(packaged, name, None)
        .ok_or_else(|| anyhow!("Goddard JavaScript REPL is missing from this Goddard build"))
}

pub fn helper_install_root() -> anyhow::Result<PathBuf> {
    Ok(dirs::data_dir()
        .ok_or_else(|| anyhow!("Application Support directory is unavailable"))?
        .join("Goddard")
        .join("Computer Use"))
}

/// Install the bundled helper as an independent, stable runtime service.
///
/// Screen Recording differs from Accessibility on macOS: it follows the
/// responsible application. A helper launched from inside Goddard's bundle is
/// therefore attributed to Goddard even though the capture API runs in the
/// helper. Launching this standalone copy through Launch Services gives the
/// helper its own TCC identity while the signed app bundle remains the source
/// shipped with Goddard.
///
/// Installs sit under a fingerprint-named directory instead of replacing one
/// shared bundle: a helper connection outlives the install that spawned it,
/// and once a running process's executable is deleted macOS can no longer
/// resolve its code identity — every TCC check it answers then fails. Side by
/// side installs keep a newer build from breaking sessions already running.
fn install_helper_app(source: &Path) -> anyhow::Result<PathBuf> {
    install_helper_app_at(source, &helper_install_root()?)
}

fn install_helper_app_at(source: &Path, install_root: &Path) -> anyhow::Result<PathBuf> {
    crate::fs_ext::create_private_dir_all(install_root)
        .with_context(|| format!("could not create {}", install_root.display()))?;
    let bundle_name = source
        .file_name()
        .ok_or_else(|| anyhow!("Computer Use helper bundle name is invalid"))?;
    let fingerprint = helper_fingerprint(source)?;
    let version_root = install_root.join(&fingerprint);
    let destination = version_root.join(bundle_name);
    if !helper_install_matches(source, &destination)? {
        if helper_executables_running(&destination) {
            bail!(
                "the Computer Use helper at {} is in use",
                destination.display()
            );
        }
        crate::fs_ext::create_private_dir_all(&version_root)
            .with_context(|| format!("could not create {}", version_root.display()))?;
        let staging = version_root.join(format!(".install-{}.app", Uuid::new_v4().simple()));
        copy_directory(source, &staging)?;
        if destination.exists() {
            let _ = fs::remove_dir_all(&destination);
        }
        fs::rename(&staging, &destination).context("could not install Computer Use helper")?;
    }
    prune_stale_helper_installs(install_root, &fingerprint);
    Ok(destination)
}

fn helper_fingerprint(source: &Path) -> anyhow::Result<String> {
    let fingerprint = fs::read_to_string(source.join(HELPER_FINGERPRINT_PATH))
        .context("Computer Use helper has no build fingerprint")?;
    let fingerprint = fingerprint.trim();
    if fingerprint.is_empty()
        || !fingerprint
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_'))
    {
        bail!("Computer Use helper fingerprint is invalid");
    }
    Ok(fingerprint.to_owned())
}

/// Remove installs nothing still executes from — including flat `.app` bundles
/// left by pre-fingerprint installs. A directory a live helper runs from stays:
/// deleting its files would break the process's TCC identity.
fn prune_stale_helper_installs(install_root: &Path, current_fingerprint: &str) {
    let Ok(entries) = fs::read_dir(install_root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        // Dotfiles also cover in-progress .install-* staging directories, which
        // a concurrent daemon may still be filling — only finished installs
        // (a directory holding a .app, or a legacy flat .app) are candidates.
        if name.starts_with('.') || name == current_fingerprint || !path.is_dir() {
            continue;
        }
        let installed = name.ends_with(".app") || contains_app_bundle(&path);
        if !installed || helper_executables_running(&path) {
            continue;
        }
        let _ = fs::remove_dir_all(&path);
    }
}

fn contains_app_bundle(directory: &Path) -> bool {
    fs::read_dir(directory).is_ok_and(|entries| {
        entries.flatten().any(|entry| {
            entry.file_type().is_ok_and(|kind| kind.is_dir())
                && entry
                    .file_name()
                    .to_str()
                    .is_some_and(|name| name.ends_with(".app"))
        })
    })
}

/// proc_pidpath keeps resolving a helper's launch path after its bundle is
/// deleted, so this check still guards orphaned helpers.
#[cfg(target_os = "macos")]
fn helper_executables_running(directory: &Path) -> bool {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    const PROC_ALL_PIDS: u32 = 1;
    let bytes = unsafe { libc::proc_listpids(PROC_ALL_PIDS, 0, std::ptr::null_mut(), 0) };
    if bytes <= 0 {
        return true;
    }
    let mut pids = vec![0 as libc::pid_t; bytes as usize / size_of::<libc::pid_t>()];
    let listed = unsafe { libc::proc_listpids(PROC_ALL_PIDS, 0, pids.as_mut_ptr().cast(), bytes) };
    if listed <= 0 {
        return true;
    }
    for pid in pids.into_iter().filter(|pid| *pid > 0) {
        let mut buffer = vec![0_u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
        let length = unsafe {
            libc::proc_pidpath(
                pid,
                buffer.as_mut_ptr().cast(),
                libc::PROC_PIDPATHINFO_MAXSIZE as u32,
            )
        };
        if length <= 0 {
            continue;
        }
        buffer.truncate(length as usize);
        if PathBuf::from(OsString::from_vec(buffer)).starts_with(directory) {
            return true;
        }
    }
    false
}

/// Helper installs only happen on macOS; other platforms never reach this.
#[cfg(not(target_os = "macos"))]
fn helper_executables_running(_directory: &Path) -> bool {
    false
}

const HELPER_FINGERPRINT_PATH: &str = "Contents/Resources/.goddard-helper-fingerprint";

fn helper_install_matches(source: &Path, destination: &Path) -> anyhow::Result<bool> {
    if !destination.is_dir() {
        return Ok(false);
    }
    let fingerprint = Path::new(HELPER_FINGERPRINT_PATH);
    let source_fingerprint = fs::read(source.join(fingerprint))?;
    let Ok(installed_fingerprint) = fs::read(destination.join(fingerprint)) else {
        return Ok(false);
    };
    Ok(source_fingerprint == installed_fingerprint)
}

fn copy_directory(source: &Path, destination: &Path) -> anyhow::Result<()> {
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
            crate::fs_ext::symlink(&fs::read_link(&source_path)?, &destination_path)?;
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

pub fn skill_root_path() -> anyhow::Result<PathBuf> {
    let resources = resources_directory(&host_executable_path()?, std::env::consts::OS)?;
    // The skills tree is the whole payload on macOS; other platforms keep it
    // inside a shared resources directory that also carries helper assets.
    let (source, name, key) = if cfg!(target_os = "macos") {
        (
            resources.join("skills"),
            "skills",
            Path::new("goddard-computer-use/SKILL.md"),
        )
    } else {
        (
            resources.clone(),
            "resources",
            Path::new("skills/goddard-computer-use/SKILL.md"),
        )
    };
    let packaged = source.is_dir().then_some(source.as_path());
    let path = staged_resource(packaged, name, Some(key))
        .map(|staged| {
            if cfg!(target_os = "macos") {
                staged
            } else {
                staged.join("skills")
            }
        })
        .unwrap_or_else(|| resources.join("skills"));
    packaged_file(
        &path.join("goddard-computer-use/SKILL.md"),
        "Goddard Computer Use skill",
    )?;
    Ok(path)
}

fn host_executable_path() -> anyhow::Result<PathBuf> {
    std::env::var_os(crate::APP_EXECUTABLE_ENV)
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .map(Ok)
        .unwrap_or_else(|| {
            std::env::current_exe().context("Goddard executable path is unavailable")
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packaged_resources_follow_each_platform_layout() {
        for (executable, os, resources) in [
            (
                "/Applications/Goddard.app/Contents/MacOS/Goddard",
                "macos",
                "/Applications/Goddard.app/Contents/Resources",
            ),
            (
                "/opt/goddard/bin/goddard",
                "linux",
                "/opt/goddard/share/goddard",
            ),
            (
                "/dev/waku/target/debug/goddard",
                "linux",
                "/dev/waku/target/debug/resources",
            ),
            ("/Goddard/goddard.exe", "windows", "/Goddard/resources"),
        ] {
            assert_eq!(
                resources_directory(Path::new(executable), os).unwrap(),
                PathBuf::from(resources)
            );
        }
    }

    #[test]
    fn app_grants_preserve_bundle_identity() {
        let target = ComputerTarget {
            window_id: 42,
            bundle_id: "net.imput.helium".into(),
            team_id: Some("S4Q33XPHB4".into()),
            app_name: "Helium".into(),
            window_title: "Window".into(),
            width: 1440,
            height: 823,
        };
        let grant = ComputerAppGrant {
            bundle_id: "net.imput.helium".into(),
            app_name: "Helium".into(),
            verified: true,
        };
        assert_eq!(target.grant_key(), grant.key());
        assert!(target.persistable());
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

    #[test]
    fn helper_installs_keyed_by_fingerprint_prune_finished_stale_roots() {
        let root = std::env::temp_dir().join(format!("helper-install-{}", Uuid::new_v4()));
        let install_root = root.join("installed");
        let first_source = bundled_helper(&root, "aaaa1111");
        let second_source = bundled_helper(&root, "bbbb2222");

        let first = install_helper_app_at(&first_source, &install_root).unwrap();
        assert_eq!(
            first,
            install_root.join("aaaa1111/Goddard Computer Use.app")
        );
        assert!(first.join("Contents/MacOS/Goddard Computer Use").is_file());

        // A different build installs beside the first, then prunes it — no
        // process executes from this test's temp directory.
        let second = install_helper_app_at(&second_source, &install_root).unwrap();
        assert_eq!(
            second,
            install_root.join("bbbb2222/Goddard Computer Use.app")
        );
        assert!(!first.exists());
        assert!(second.exists());

        // Reinstalling the same fingerprint reuses it; a flat pre-fingerprint
        // install prunes like any other stale root, while staging leftovers
        // and unrelated directories stay.
        let legacy = install_root.join("Goddard Computer Use.app");
        let staging = install_root.join(".install-leftover.app");
        let unrelated = install_root.join("unrelated");
        for directory in [&legacy, &staging, &unrelated] {
            fs::create_dir_all(directory).unwrap();
        }
        let again = install_helper_app_at(&second_source, &install_root).unwrap();
        assert_eq!(again, second);
        assert!(!legacy.exists());
        assert!(staging.exists());
        assert!(unrelated.exists());
        assert!(second.exists());

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn preview_updates_restore_the_pip_state() {
        let state = decode_preview_update(
            br#"{
                "target": {
                    "windowId": 42,
                    "bundleId": "net.imput.helium",
                    "teamId": "S4Q33XPHB4",
                    "appName": "Helium",
                    "windowTitle": "Window",
                    "width": 1440,
                    "height": 823
                },
                "imageUrl": "data:image/png;base64,aGVsbG8="
            }"#,
        )
        .unwrap();

        assert_eq!(state.target.unwrap().window_id, 42);
        assert_eq!(state.phase, ComputerUsePhase::Running);
        assert!(state.visible);
        assert!(state.image_url.is_some());
    }
}
