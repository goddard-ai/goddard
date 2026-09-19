//! Credential storage for integrations: macOS Keychain via `security`, with
//! a permission-locked file fallback for other platforms and for hosts where
//! the keychain write fails.

use std::fs;
use std::io::Write as _;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use anyhow::{Context as _, anyhow};

const KEYCHAIN_SERVICE: &str = "ai.goddard.integrations";

#[derive(Clone)]
pub struct SecretStore {
    fallback_dir: PathBuf,
}

impl SecretStore {
    pub fn new(data_dir: PathBuf) -> Self {
        Self {
            fallback_dir: data_dir.join("secrets"),
        }
    }

    pub fn read(&self, key: &str) -> Option<String> {
        #[cfg(target_os = "macos")]
        if let Some(value) = keychain_read(key) {
            return Some(value);
        }
        let path = self.fallback_path(key);
        fs::read_to_string(&path).ok().filter(|s| !s.is_empty())
    }

    pub fn store(&self, key: &str, value: &str) -> anyhow::Result<()> {
        #[cfg(target_os = "macos")]
        if keychain_store(key, value).is_ok() {
            self.remove_fallback(key);
            return Ok(());
        }
        self.store_fallback(key, value)
    }

    pub fn remove(&self, key: &str) {
        #[cfg(target_os = "macos")]
        keychain_remove(key);
        self.remove_fallback(key);
    }

    fn fallback_path(&self, key: &str) -> PathBuf {
        self.fallback_dir.join(format!("{key}.secret"))
    }

    fn store_fallback(&self, key: &str, value: &str) -> anyhow::Result<()> {
        crate::fs_ext::create_private_dir_all(&self.fallback_dir)?;
        let path = self.fallback_path(key);
        fs::write(&path, value)
            .with_context(|| format!("could not write credential {}", path.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
        }
        Ok(())
    }

    fn remove_fallback(&self, key: &str) {
        let _ = fs::remove_file(self.fallback_path(key));
    }
}

/// `security -i` reads commands from stdin, which keeps secret values out of
/// the process list. `security` is on the item's ACL, so reads and updates
/// do not prompt.
#[cfg(target_os = "macos")]
fn security_i(command: &str) -> anyhow::Result<std::process::Output> {
    let mut child = Command::new("/usr/bin/security")
        .arg("-i")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("could not run `security`")?;
    child
        .stdin
        .take()
        .ok_or_else(|| anyhow!("`security` stdin unavailable"))?
        .write_all(command.as_bytes())
        .context("could not write `security` command")?;
    child
        .wait_with_output()
        .context("could not read `security` output")
}

/// Quote a value for `security -i`'s command parser.
#[cfg(target_os = "macos")]
fn quoted(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

#[cfg(target_os = "macos")]
fn keychain_read(key: &str) -> Option<String> {
    let command = format!(
        "find-generic-password -s {} -a {} -w\n",
        quoted(KEYCHAIN_SERVICE),
        quoted(key)
    );
    let output = security_i(&command).ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    (!value.is_empty()).then_some(value)
}

#[cfg(target_os = "macos")]
fn keychain_store(key: &str, value: &str) -> anyhow::Result<()> {
    let command = format!(
        "add-generic-password -U -s {} -a {} -w {}\n",
        quoted(KEYCHAIN_SERVICE),
        quoted(key),
        quoted(value)
    );
    let output = security_i(&command)?;
    if !output.status.success() {
        return Err(anyhow!(
            "`security` add-generic-password failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn keychain_remove(key: &str) {
    let command = format!(
        "delete-generic-password -s {} -a {}\n",
        quoted(KEYCHAIN_SERVICE),
        quoted(key)
    );
    let _ = security_i(&command);
}
