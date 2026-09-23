//! Computer Use helper process integration.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, SystemTime};

use anyhow::Context as _;
use parking_lot::Mutex;
use uuid::Uuid;
use waku_protocol::computer_use::ComputerApprovalId;
use waku_protocol::model::PermissionOption;

use crate::computer_use;
use crate::driver::DriverEventSender;
use crate::fs_ext;
use crate::model::DriverEvent;

#[cfg(target_os = "macos")]
use std::ffi::OsString;
#[cfg(target_os = "macos")]
use std::os::unix::ffi::OsStringExt as _;

#[derive(Clone)]
pub(super) struct ComputerUseConfig {
    pub(super) server_path: PathBuf,
    pub(super) repl_path: PathBuf,
    pub(super) skill_path: PathBuf,
    pub(super) process_directory: PathBuf,
}

impl ComputerUseConfig {
    pub(super) fn mcp_server(&self) -> serde_json::Value {
        serde_json::json!({
            "command": self.repl_path,
            "args": [],
            "env": {
                "GODDARD_COMPUTER_USE_SERVER": self.server_path,
                "GODDARD_COMPUTER_USE_PROCESS_DIRECTORY": self.process_directory,
            }
        })
    }
}

pub(super) struct ComputerUseRuntime {
    pub(super) config: ComputerUseConfig,
    preview_monitor: Option<ComputerUsePreviewMonitor>,
}

impl ComputerUseRuntime {
    pub(super) fn start(events: DriverEventSender) -> anyhow::Result<Self> {
        let server_path = computer_use::mcp_server_command()?;
        let repl_path = computer_use::js_repl_server_path()?;
        let skill_path = computer_use::skill_root_path()?
            .join("goddard-computer-use")
            .join("SKILL.md");
        let process_directory = create_process_directory()?;
        let preview_monitor =
            match ComputerUsePreviewMonitor::start(process_directory.clone(), events) {
                Ok(monitor) => monitor,
                Err(error) => {
                    let _ = fs::remove_dir_all(&process_directory);
                    return Err(error);
                }
            };
        Ok(Self {
            config: ComputerUseConfig {
                server_path,
                repl_path,
                skill_path,
                process_directory,
            },
            preview_monitor: Some(preview_monitor),
        })
    }

    pub(super) fn stop(&self) {
        stop_registered_processes(&self.config.process_directory, &self.config.server_path);
    }
}

impl Drop for ComputerUseRuntime {
    fn drop(&mut self) {
        self.stop();
        drop(self.preview_monitor.take());
        process_directories()
            .lock()
            .remove(&self.config.process_directory);
        let _ = fs::remove_dir_all(&self.config.process_directory);
    }
}

fn process_directories() -> &'static Mutex<HashSet<PathBuf>> {
    static DIRECTORIES: OnceLock<Mutex<HashSet<PathBuf>>> = OnceLock::new();
    DIRECTORIES.get_or_init(|| Mutex::new(HashSet::new()))
}

pub(super) fn set_enabled_for_runtimes(enabled: bool) {
    let helper = computer_use::mcp_server_command().ok();
    process_directories().lock().retain(|directory| {
        if !directory.is_dir() {
            return false;
        }
        let marker = directory.join("computer-use-disabled");
        if enabled {
            let _ = fs::remove_file(marker);
        } else {
            let _ = fs::write(marker, b"");
            if let Some(helper) = helper.as_deref() {
                stop_registered_processes(directory, helper);
            }
        }
        true
    });
}

pub(super) fn revoke_app_grants_for_runtimes() {
    let revision = Uuid::new_v4().simple().to_string();
    let helper = computer_use::mcp_server_command().ok();
    process_directories().lock().retain(|directory| {
        if !directory.is_dir() {
            return false;
        }
        let _ = fs::write(directory.join("computer-use-grants-revision"), &revision);
        if let Some(helper) = helper.as_deref() {
            stop_registered_processes(directory, helper);
        }
        true
    });
}

pub(super) struct ComputerUsePreviewMonitor {
    running: Arc<AtomicBool>,
    directory: PathBuf,
}

fn pending_approvals() -> &'static Mutex<HashMap<String, PathBuf>> {
    static PENDING: OnceLock<Mutex<HashMap<String, PathBuf>>> = OnceLock::new();
    PENDING.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Computer Use approvals use the same response command as provider prompts.
/// The REPL waits on this file while the provider's tool call is suspended.
pub(super) fn respond_approval(request_id: &str, decision: &str) -> bool {
    if ComputerApprovalId::decode(request_id).is_none() {
        return false;
    }
    if let Some(path) = pending_approvals().lock().remove(request_id) {
        let _ = fs::write(path, decision);
    }
    true
}

impl ComputerUsePreviewMonitor {
    pub(super) fn start(directory: PathBuf, events: DriverEventSender) -> anyhow::Result<Self> {
        let running = Arc::new(AtomicBool::new(true));
        let thread_running = running.clone();
        let thread_directory = directory.clone();
        thread::Builder::new()
            .name("goddard-computer-use-preview".into())
            .spawn(move || {
                let directory = thread_directory;
                let mut seen = HashMap::<PathBuf, (SystemTime, u64)>::new();
                while thread_running.load(Ordering::Acquire) {
                    if let Ok(entries) = fs::read_dir(&directory) {
                        for entry in entries.flatten() {
                            let path = entry.path();
                            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                                continue;
                            };
                            if !name.ends_with(".json")
                                || (!name.starts_with("preview-")
                                    && !name.starts_with("approval-request-"))
                            {
                                continue;
                            }
                            let Ok(metadata) = entry.metadata() else {
                                continue;
                            };
                            let Ok(modified) = metadata.modified() else {
                                continue;
                            };
                            let revision = (modified, metadata.len());
                            if seen.get(&path) == Some(&revision) {
                                continue;
                            }
                            seen.insert(path.clone(), revision);
                            let Ok(data) = fs::read(&path) else {
                                continue;
                            };
                            if name.starts_with("approval-request-") {
                                if let Ok(approval) =
                                    serde_json::from_slice::<ComputerApprovalId>(&data)
                                {
                                    let request_id = approval.encode();
                                    let response = directory
                                        .join(format!("approval-response-{}.txt", approval.nonce));
                                    pending_approvals()
                                        .lock()
                                        .insert(request_id.clone(), response);
                                    let persistent = approval.app_grant().is_some();
                                    let mut options = vec![PermissionOption::keyed(
                                        "task",
                                        localized!("computer_use.allow_for_task"),
                                        true,
                                    )];
                                    if persistent {
                                        options.push(PermissionOption::keyed(
                                            "always",
                                            localized!("computer_use.always_allow_app"),
                                            true,
                                        ));
                                    }
                                    options.push(PermissionOption::keyed(
                                        "deny",
                                        localized!("common.deny"),
                                        false,
                                    ));
                                    let (title, title_i18n) = localized!(
                                        "computer_use.allow_control",
                                        app = approval.app_name.clone()
                                    );
                                    let (detail, detail_i18n) = if approval.scope == "clipboard" {
                                        localized!("computer_use.approval_clipboard")
                                    } else if approval.scope == "desktop" {
                                        localized!("computer_use.approval_desktop")
                                    } else if approval.scope == "browser" {
                                        localized!("computer_use.approval_browser")
                                    } else {
                                        localized!(
                                            "computer_use.approval_app",
                                            app = approval.app_name.clone()
                                        )
                                    };
                                    let _ = events.send(DriverEvent::Permission {
                                        request_id,
                                        title,
                                        title_i18n: Some(title_i18n),
                                        detail,
                                        detail_i18n: Some(detail_i18n),
                                        options,
                                    });
                                }
                            } else if let Ok(state) = computer_use::decode_preview_update(&data) {
                                let _ = events.send(DriverEvent::ComputerUseUpdated(state));
                            }
                        }
                    }
                    thread::sleep(Duration::from_millis(50));
                }
            })
            .context("failed to start the Computer Use preview monitor")?;
        Ok(Self { running, directory })
    }
}

impl Drop for ComputerUsePreviewMonitor {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Release);
        pending_approvals()
            .lock()
            .retain(|_, path| !path.starts_with(&self.directory));
    }
}

pub(super) fn create_process_directory() -> anyhow::Result<PathBuf> {
    let directory = std::env::temp_dir()
        .join("goddard-computer-use")
        .join(Uuid::new_v4().simple().to_string());
    fs::create_dir_all(&directory).with_context(|| {
        format!(
            "could not create Computer Use process directory {}",
            directory.display()
        )
    })?;
    fs_ext::restrict_to_owner(&directory).with_context(|| {
        format!(
            "could not secure Computer Use process directory {}",
            directory.display()
        )
    })?;
    process_directories().lock().insert(directory.clone());
    Ok(directory)
}

pub(super) fn stop_registered_processes(directory: &Path, helper_executable: &Path) {
    // An in-flight `js` call holds the kernel's serve loop and cannot see a
    // helper die, so the kernel polls this marker itself. Each `tools/call`
    // clears it on entry. The name is a wire contract with `goddard_js_repl`
    // (src/js_repl.rs), which lives outside this crate.
    let _ = fs::write(directory.join("cancel-kernel"), b"");
    let expected_executable =
        dunce::canonicalize(helper_executable).unwrap_or_else(|_| helper_executable.to_path_buf());
    for (pid, registration) in registered_processes(directory) {
        if process_executable(pid).as_deref() == Some(expected_executable.as_path()) {
            // macOS owns a Launch Services bridge; closing it interrupts the
            // native SDK. Portable hosts poll a cancellation marker so they
            // can cancel the operation and await SDK shutdown on both OSes.
            #[cfg(target_os = "macos")]
            unsafe {
                libc::kill(pid, libc::SIGTERM);
            }
            #[cfg(not(target_os = "macos"))]
            let _ = fs::write(directory.join(format!("cancel-{pid}")), b"");
        }
        let _ = fs::remove_file(registration);
    }
}

pub(super) fn registered_processes(directory: &Path) -> Vec<(i32, PathBuf)> {
    let Ok(entries) = fs::read_dir(directory) else {
        return Vec::new();
    };
    entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            if !entry.file_type().ok()?.is_file() {
                return None;
            }
            let pid = entry.file_name().to_str()?.parse::<i32>().ok()?;
            (pid > 1).then_some((pid, entry.path()))
        })
        .collect()
}

#[cfg(target_os = "macos")]
pub(super) fn process_executable(pid: i32) -> Option<PathBuf> {
    let mut buffer = vec![0_u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
    let length = unsafe {
        libc::proc_pidpath(
            pid,
            buffer.as_mut_ptr().cast(),
            libc::PROC_PIDPATHINFO_MAXSIZE as u32,
        )
    };
    if length <= 0 {
        return None;
    }
    buffer.truncate(length as usize);
    Some(PathBuf::from(OsString::from_vec(buffer)))
}

#[cfg(target_os = "linux")]
pub(super) fn process_executable(pid: i32) -> Option<PathBuf> {
    fs::read_link(format!("/proc/{pid}/exe")).ok()
}

#[cfg(target_os = "windows")]
pub(super) fn process_executable(pid: i32) -> Option<PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
    };
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid as u32);
        if process.is_null() {
            return None;
        }
        let mut buffer = vec![0u16; 32768];
        let mut len = buffer.len() as u32;
        let success = QueryFullProcessImageNameW(process, 0, buffer.as_mut_ptr(), &mut len);
        CloseHandle(process);
        (success != 0)
            .then(|| PathBuf::from(std::ffi::OsString::from_wide(&buffer[..len as usize])))
            .and_then(|path| dunce::canonicalize(path).ok())
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
pub(super) fn process_executable(_: i32) -> Option<PathBuf> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn approval_request_reaches_permission_event_and_response_file() {
        let directory =
            std::env::temp_dir().join(format!("goddard-computer-approval-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap();
        let (events, received) = crate::driver::test_event_channel();
        let monitor = ComputerUsePreviewMonitor::start(directory.clone(), events).unwrap();
        let approval = ComputerApprovalId {
            nonce: Uuid::new_v4().simple().to_string(),
            scope: "app:com.example.Editor".into(),
            app_name: "Editor".into(),
            bundle_id: Some("com.example.Editor".into()),
        };
        fs::write(
            directory.join(format!("approval-request-{}.json", approval.nonce)),
            serde_json::to_vec(&approval).unwrap(),
        )
        .unwrap();
        let event = received.recv_timeout(Duration::from_secs(2)).unwrap();
        let DriverEvent::Permission {
            request_id,
            options,
            ..
        } = event
        else {
            panic!("expected a permission event");
        };
        assert_eq!(request_id, approval.encode());
        assert!(options.iter().any(|option| option.id == "always"));
        assert!(respond_approval(&request_id, "always"));
        let response = directory.join(format!("approval-response-{}.txt", approval.nonce));
        assert_eq!(fs::read_to_string(response).unwrap(), "always");
        drop(monitor);
        fs::remove_dir_all(directory).unwrap();
    }
}
