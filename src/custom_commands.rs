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

use crate::persistence::{CustomCommand, CustomCommandIcon};

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

/// Title prefix the launch line's sentinel uses to hand the script's exit
/// code back to the app: the line ends by writing an OSC 2 sequence whose
/// title is `{prefix}{code}`, and `TerminalEventProxy` swallows it before
/// it can rename the tab. This is the only completion signal available —
/// the shell stays interactive after the script, so nothing exits.
const COMMAND_EXIT_TITLE_PREFIX: &str = "waku-command-exit:";

/// A sentinel title's reported exit code, if this title is one.
pub fn parse_command_exit(title: &str) -> Option<i32> {
    title
        .strip_prefix(COMMAND_EXIT_TITLE_PREFIX)
        .and_then(|code| code.trim().parse().ok())
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
/// that persists — then report the script's exit code through an invisible
/// OSC 2 title sentinel and, when the command is configured to close on
/// success, exit the shell so the tab can close. A failed script leaves the
/// shell open with its output visible.
pub fn source_line(shell: &Path, script_path: &Path, close_on_success: bool) -> String {
    let quoted = shell_quote(script_path);
    let name = shell_file_name(shell);
    if name == "nu" || name == "nu.exe" {
        let report = |code: &str| {
            format!(
                "print --no-newline $\"(char esc)]2;{COMMAND_EXIT_TITLE_PREFIX}({code})(char esc)\\\\\""
            )
        };
        return if close_on_success {
            format!(
                "source {quoted}; let waku_status = $env.LAST_EXIT_CODE; {}; if $waku_status == 0 {{ exit }}",
                report("$waku_status")
            )
        } else {
            format!("source {quoted}; {}", report("$env.LAST_EXIT_CODE"))
        };
    }
    if name.starts_with("pwsh") || name.starts_with("powershell") {
        // `$?` — `$LASTEXITCODE` only tracks native commands — reflects the
        // script's last statement, so the sentinel reports 0 or 1 rather
        // than a real code, and `&&` needs PowerShell 7 anyway.
        let report = |code: &str| {
            format!(
                "[Console]::Write(\"$([char]27)]2;{COMMAND_EXIT_TITLE_PREFIX}$({code})$([char]27)\\\")"
            )
        };
        return if close_on_success {
            format!(
                ". {quoted}; $waku_ok = $?; {}; if ($waku_ok) {{ exit }}",
                report("[int](-not $waku_ok)")
            )
        } else {
            format!(". {quoted}; {}", report("[int](-not $?)"))
        };
    }
    if name == "cmd" || name == "cmd.exe" {
        // cmd cannot emit the sentinel; Windows keeps the reveal-on-run
        // behavior and never waits on a report.
        let quoted = format!("\"{}\"", script_path.to_string_lossy());
        return if close_on_success {
            format!("call {quoted} && exit")
        } else {
            format!("call {quoted}")
        };
    }
    // `source` is fish's spelling; `.` is the POSIX one. `&&` chains in fish
    // ≥ 3.0 and every POSIX shell. The printf emits the sentinel as an
    // ST-terminated OSC 2, which never reaches the screen.
    let report = |code: &str| {
        format!("printf '\\033]2;{COMMAND_EXIT_TITLE_PREFIX}%s\\033\\\\' {code}")
    };
    if name.starts_with("fish") {
        return if close_on_success {
            format!(
                "source {quoted}; set waku_status $status; {}; [ $waku_status -eq 0 ] && exit",
                report("$waku_status")
            )
        } else {
            format!("source {quoted}; {}", report("$status"))
        };
    }
    let source = format!(". {quoted}");
    if close_on_success {
        format!(
            "{source}; waku_status=$?; {}; [ \"$waku_status\" -eq 0 ] && exit",
            report("\"$waku_status\"")
        )
    } else {
        format!("{source}; {}", report("\"$?\""))
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
    fn source_line_matches_the_shell_family() {
        let path = Path::new("/tmp/command-abc");
        let sentinel = "\\033]2;waku-command-exit:%s\\033\\\\";
        assert_eq!(
            source_line(Path::new("/bin/zsh"), path, false),
            format!(". '/tmp/command-abc'; printf '{sentinel}' \"$?\"")
        );
        assert_eq!(
            source_line(Path::new("/bin/zsh"), path, true),
            format!(
                ". '/tmp/command-abc'; waku_status=$?; printf '{sentinel}' \"$waku_status\"; [ \"$waku_status\" -eq 0 ] && exit"
            )
        );
        assert_eq!(
            source_line(Path::new("/opt/homebrew/bin/fish"), path, false),
            format!("source '/tmp/command-abc'; printf '{sentinel}' $status")
        );
        assert_eq!(
            source_line(Path::new("/usr/local/bin/nu"), path, true),
            "source '/tmp/command-abc'; let waku_status = $env.LAST_EXIT_CODE; \
             print --no-newline $\"(char esc)]2;waku-command-exit:($waku_status)(char esc)\\\\\"; \
             if $waku_status == 0 { exit }"
        );
        assert_eq!(
            source_line(
                Path::new("C:/Program Files/PowerShell/7/pwsh.exe"),
                path,
                true
            ),
            ". '/tmp/command-abc'; $waku_ok = $?; [Console]::Write(\"$([char]27)]2;waku-command-exit:$([int](-not $waku_ok))$([char]27)\\\"); if ($waku_ok) { exit }"
        );
        assert_eq!(
            source_line(Path::new("C:/Windows/System32/cmd.exe"), path, true),
            "call \"/tmp/command-abc\" && exit"
        );
    }

    #[test]
    fn parse_command_exit_reads_only_sentinel_titles() {
        assert_eq!(parse_command_exit("waku-command-exit:0"), Some(0));
        assert_eq!(parse_command_exit("waku-command-exit:127"), Some(127));
        assert_eq!(parse_command_exit("waku-command-exit:1"), Some(1));
        assert_eq!(parse_command_exit("waku-command-exit:"), None);
        assert_eq!(parse_command_exit("waku-command-exit:abc"), None);
        assert_eq!(parse_command_exit("vim — ~/project"), None);
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

    #[test]
    fn source_line_escapes_single_quotes() {
        let path = Path::new("/tmp/it's/command");
        assert_eq!(
            source_line(Path::new("/bin/sh"), path, false),
            r#". '/tmp/it'\''s/command'; printf '\033]2;waku-command-exit:%s\033\\' "$?""#
        );
    }
}
