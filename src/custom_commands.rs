//! Materialization for user-defined custom terminal commands.
//!
//! A command's `script` is written to a file named by a hash of its text, so
//! repeat invocations — and different commands carrying the same script —
//! share one file instead of each writing their own. The terminal then
//! sources that file inside an interactive shell, which makes the command
//! behave exactly like text typed at the prompt.
//!
//! This file path is the fallback: a shell with integration hooks reports
//! command boundaries itself, so a single-line script runs as plain typed
//! input with no materialized file and no exit sentinel. Multi-line scripts
//! would report each line as its own command, and shells with no hook
//! surface report nothing — both keep the sourced file below.

use std::fs;
use std::io;
use std::path::PathBuf;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt as _;

use crate::persistence::{CustomCommand, CustomCommandIcon};

pub use waku_protocol::terminal_command::{parse_command_exit, shell_quote, source_line};

/// The file a script's hash maps to, whether or not it exists yet.
pub fn script_path(script: &str) -> PathBuf {
    waku_protocol::terminal_command::script_path(
        &crate::persistence::custom_command_scripts_directory(),
        script,
    )
}

/// Write the script to its hashed file unless a file already holds exactly
/// these bytes — the reuse case. The write is small and happens on a one-shot
/// user action, so it stays synchronous.
pub fn ensure_script(script: &str) -> io::Result<PathBuf> {
    let path = script_path(script);
    if fs::read(&path).is_ok_and(|bytes| bytes == script.as_bytes()) {
        return Ok(path);
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&path, script)?;
    #[cfg(unix)]
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
    Ok(path)
}

/// Delete the hashed script file once no configured command still carries
/// that script — called after an edit or delete. Missing files are fine.
pub fn remove_script_if_unreferenced(script: &str, commands: &[CustomCommand]) {
    if commands.iter().any(|command| command.script == script) {
        return;
    }
    let _ = fs::remove_file(script_path(script));
}

/// The shell a command runs in: its configured override when set, otherwise
/// the same default the plain terminal uses.
pub fn command_shell(command: &CustomCommand) -> PathBuf {
    command
        .shell
        .as_deref()
        .map(str::trim)
        .filter(|shell| !shell.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(crate::command_env::default_terminal_shell)
}

/// The bundled asset a command's icon choice renders from — the command
/// palette row, the settings list, and its terminal tab all share it.
pub fn icon_path(icon: CustomCommandIcon) -> &'static str {
    match icon {
        CustomCommandIcon::Terminal => "icons/terminal.svg",
        CustomCommandIcon::Command => "icons/command.svg",
        CustomCommandIcon::Zap => "icons/zap.svg",
        CustomCommandIcon::Wrench => "icons/wrench.svg",
        CustomCommandIcon::Gauge => "icons/gauge.svg",
        CustomCommandIcon::Package => "icons/package.svg",
        CustomCommandIcon::GitBranch => "icons/git-branch.svg",
        CustomCommandIcon::GitHub => "icons/github.svg",
        CustomCommandIcon::Folder => "icons/folder.svg",
        CustomCommandIcon::File => "icons/file.svg",
        CustomCommandIcon::Search => "icons/search.svg",
        CustomCommandIcon::Globe => "icons/globe.svg",
        CustomCommandIcon::Server => "icons/server.svg",
        CustomCommandIcon::CloudUpload => "icons/cloud-upload.svg",
        CustomCommandIcon::Download => "icons/download.svg",
        CustomCommandIcon::Bot => "icons/bot.svg",
        CustomCommandIcon::Sparkle => "icons/sparkle.svg",
        CustomCommandIcon::Star => "icons/star.svg",
        CustomCommandIcon::Target => "icons/target.svg",
        CustomCommandIcon::Queue => "icons/queue.svg",
        CustomCommandIcon::Compose => "icons/compose.svg",
        CustomCommandIcon::Chart => "icons/chart-column.svg",
        CustomCommandIcon::Refresh => "icons/rotate-cw.svg",
        CustomCommandIcon::Archive => "icons/archive.svg",
        CustomCommandIcon::Bug => "icons/bug.svg",
        CustomCommandIcon::Languages => "icons/languages.svg",
        CustomCommandIcon::Database => "icons/database.svg",
        CustomCommandIcon::CircleCheck => "icons/circle-check.svg",
        CustomCommandIcon::Beaker => "icons/beaker.svg",
        CustomCommandIcon::GitMerge => "icons/git-merge.svg",
        CustomCommandIcon::Eye => "icons/eye.svg",
        CustomCommandIcon::Pencil => "icons/pencil.svg",
        CustomCommandIcon::FolderSearch => "icons/folder-search.svg",
        CustomCommandIcon::FileText => "icons/file-text.svg",
        CustomCommandIcon::Lock => "icons/lock.svg",
        CustomCommandIcon::Fork => "icons/fork.svg",
        CustomCommandIcon::Brain => "icons/brain.svg",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn same_script_maps_to_one_path() {
        assert_eq!(script_path("git status"), script_path("git status"));
        assert_ne!(script_path("git status"), script_path("git diff"));
    }

    #[test]
    fn ensure_script_writes_once_and_reuses() {
        let script = format!("echo {}", Uuid::new_v4());
        let path = ensure_script(&script).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), script);
        let written = fs::metadata(&path).unwrap().modified().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(10));
        ensure_script(&script).unwrap();
        assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), written);
        fs::remove_file(path).ok();
    }

    #[test]
    fn remove_only_when_no_command_references_the_script() {
        let script = format!("echo {}", Uuid::new_v4());
        let path = ensure_script(&script).unwrap();

        let referenced = CustomCommand::new(script.clone());
        remove_script_if_unreferenced(&script, &[referenced]);
        assert!(path.exists());

        remove_script_if_unreferenced(&script, &[]);
        assert!(!path.exists());
    }

    #[test]
    fn every_command_icon_is_embedded() {
        use crate::assets::Assets;
        use gpui::AssetSource;

        for icon in CustomCommandIcon::ALL {
            assert!(
                Assets.load(icon_path(icon)).unwrap().is_some(),
                "missing embedded icon: {}",
                icon_path(icon)
            );
        }
    }
}
