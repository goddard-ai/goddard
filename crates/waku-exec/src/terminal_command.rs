//! Host-side preparation for custom terminal commands.

use std::fs;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt as _;

use waku_protocol::custom_commands::CustomCommand;
use waku_protocol::terminal_command::script_path;

/// Publish complete, private scripts atomically so simultaneous clients
/// cannot source a file while another launch is still writing it.
pub(crate) fn ensure_script(directory: &Path, script: &str) -> io::Result<PathBuf> {
    let path = script_path(directory, script);
    if fs::read(&path).is_ok_and(|bytes| bytes == script.as_bytes()) {
        return Ok(path);
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temporary = directory.join(format!(".command-{}", uuid::Uuid::new_v4()));
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    let result = (|| {
        let mut file = options.open(&temporary)?;
        file.write_all(script.as_bytes())?;
        drop(file);
        fs::rename(&temporary, &path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result?;
    Ok(path)
}

/// The shell a command runs in: its configured override when set, otherwise
/// the same default the plain terminal uses.
pub(crate) fn command_shell(command: &CustomCommand) -> PathBuf {
    command
        .shell
        .as_deref()
        .map(str::trim)
        .filter(|shell| !shell.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(crate::command_env::default_terminal_shell)
}
