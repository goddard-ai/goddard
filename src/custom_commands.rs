//! Materialization for user-defined custom terminal commands.
//!
//! A command's `script` is written to a file named by a hash of its text, so
//! repeat invocations — and different commands carrying the same script —
//! share one file instead of each writing their own. The terminal then
//! sources that file inside an interactive shell, which makes the command
//! behave exactly like text typed at the prompt.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt as _;

use crate::persistence::CustomCommand;

/// FNV-1a over the script bytes. Only the file name depends on it — a
/// collision between two different scripts degrades to a rewrite per run,
/// never to running the wrong text.
fn script_hash(script: &str) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut hash = OFFSET;
    for byte in script.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(PRIME);
    }
    hash
}

/// The file a script's hash maps to, whether or not it exists yet.
pub fn script_path(script: &str) -> PathBuf {
    crate::persistence::custom_command_scripts_directory()
        .join(format!("command-{:016x}", script_hash(script)))
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

/// `'…'` quoting for the path inside a shell command line.
fn shell_quote(path: &Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', r"'\''"))
}

fn shell_file_name(shell: &Path) -> String {
    shell
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
}

/// The line fed to the interactive shell: source the materialized script so
/// it runs with every capability typed input has — aliases, functions, `cd`
/// that persists — then, when the command is configured to close on success,
/// exit the shell so the tab can close. A failed script leaves the shell
/// open with its output visible.
pub fn source_line(shell: &Path, script_path: &Path, close_on_success: bool) -> String {
    let quoted = shell_quote(script_path);
    let name = shell_file_name(shell);
    if name == "nu" || name == "nu.exe" {
        return if close_on_success {
            format!("source {quoted}; if $env.LAST_EXIT_CODE == 0 {{ exit }}")
        } else {
            format!("source {quoted}")
        };
    }
    if name.starts_with("pwsh") || name.starts_with("powershell") {
        // `$?` — `$LASTEXITCODE` only tracks native commands — reflects the
        // script's last statement, and `&&` needs PowerShell 7 anyway.
        return if close_on_success {
            format!(". {quoted}; if ($?) {{ exit }}")
        } else {
            format!(". {quoted}")
        };
    }
    if name == "cmd" || name == "cmd.exe" {
        let quoted = format!("\"{}\"", script_path.to_string_lossy());
        return if close_on_success {
            format!("call {quoted} && exit")
        } else {
            format!("call {quoted}")
        };
    }
    // `source` is fish's spelling; `.` is the POSIX one. `&&` chains in fish
    // ≥ 3.0 and every POSIX shell.
    let source = if name.starts_with("fish") {
        format!("source {quoted}")
    } else {
        format!(". {quoted}")
    };
    if close_on_success {
        format!("{source} && exit")
    } else {
        source
    }
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
    fn source_line_matches_the_shell_family() {
        let path = Path::new("/tmp/command-abc");
        assert_eq!(
            source_line(Path::new("/bin/zsh"), path, false),
            ". '/tmp/command-abc'"
        );
        assert_eq!(
            source_line(Path::new("/bin/zsh"), path, true),
            ". '/tmp/command-abc' && exit"
        );
        assert_eq!(
            source_line(Path::new("/opt/homebrew/bin/fish"), path, false),
            "source '/tmp/command-abc'"
        );
        assert_eq!(
            source_line(Path::new("/usr/local/bin/nu"), path, true),
            "source '/tmp/command-abc'; if $env.LAST_EXIT_CODE == 0 { exit }"
        );
        assert_eq!(
            source_line(
                Path::new("C:/Program Files/PowerShell/7/pwsh.exe"),
                path,
                true
            ),
            ". '/tmp/command-abc'; if ($?) { exit }"
        );
        assert_eq!(
            source_line(Path::new("C:/Windows/System32/cmd.exe"), path, true),
            "call \"/tmp/command-abc\" && exit"
        );
    }

    #[test]
    fn source_line_escapes_single_quotes() {
        let path = Path::new("/tmp/it's/command");
        assert_eq!(
            source_line(Path::new("/bin/sh"), path, false),
            r". '/tmp/it'\''s/command'"
        );
    }
}
