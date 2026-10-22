//! Pure script naming and shell framing shared by terminal hosts and clients.

use std::path::{Path, PathBuf};

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
pub fn script_path(directory: &Path, script: &str) -> PathBuf {
    directory.join(format!("command-{:016x}", script_hash(script)))
}

/// Title prefix the launch line's sentinel uses to hand the script's exit
/// code back to the app: the line ends by writing an OSC 2 sequence whose
/// title is `{prefix}{code}`, and `TerminalEventProxy` swallows it before
/// it can rename the tab. This is the only completion signal available —
/// the shell stays interactive after the script, so nothing exits.
const COMMAND_EXIT_TITLE_PREFIX: &str = "goddard-command-exit:";

/// A sentinel title's reported exit code, if this title is one. Command
/// scripts materialized by pre-Goddard builds still emit `waku-command-exit:`.
pub fn parse_command_exit(title: &str) -> Option<i32> {
    title
        .strip_prefix(COMMAND_EXIT_TITLE_PREFIX)
        .or_else(|| title.strip_prefix("waku-command-exit:"))
        .and_then(|code| code.trim().parse().ok())
}

/// `'…'` quoting for the path inside a shell command line.
pub fn shell_quote(path: &Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', r"'\''"))
}

fn shell_file_name(shell: &Path) -> String {
    shell
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
}

/// The line fed to the interactive shell when the script can't run as
/// plain typed input: print the materialized script so its text is on
/// record in the scrollback, source it so it runs with every capability
/// typed input has — aliases, functions, `cd` that persists — then report
/// the script's exit code through an invisible OSC 2 title sentinel and,
/// when the command is configured to close on success, exit the shell so
/// the tab can close. A failed script leaves the shell open with its
/// output visible.
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
                "open --raw {quoted}; source {quoted}; let term_status = $env.LAST_EXIT_CODE; {}; if $term_status == 0 {{ exit }}",
                report("$term_status")
            )
        } else {
            format!(
                "open --raw {quoted}; source {quoted}; {}",
                report("$env.LAST_EXIT_CODE")
            )
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
                "Get-Content {quoted}; . {quoted}; $term_ok = $?; {}; if ($term_ok) {{ exit }}",
                report("[int](-not $term_ok)")
            )
        } else {
            format!(
                "Get-Content {quoted}; . {quoted}; {}",
                report("[int](-not $?)")
            )
        };
    }
    if name == "cmd" || name == "cmd.exe" {
        // cmd cannot emit the sentinel; Windows keeps the reveal-on-run
        // behavior and never waits on a report.
        let quoted = format!("\"{}\"", script_path.to_string_lossy());
        return if close_on_success {
            format!("type {quoted} & call {quoted} && exit")
        } else {
            format!("type {quoted} & call {quoted}")
        };
    }
    // `source` is fish's spelling; `.` is the POSIX one. `&&` chains in fish
    // ≥ 3.0 and every POSIX shell. The printf emits the sentinel as an
    // ST-terminated OSC 2, which never reaches the screen.
    let report =
        |code: &str| format!("printf '\\033]2;{COMMAND_EXIT_TITLE_PREFIX}%s\\033\\\\' {code}");
    if name.starts_with("fish") {
        return if close_on_success {
            format!(
                "cat {quoted}; source {quoted}; set term_status $status; {}; [ $term_status -eq 0 ] && exit",
                report("$term_status")
            )
        } else {
            format!("cat {quoted}; source {quoted}; {}", report("$status"))
        };
    }
    let line = format!("cat {quoted}; . {quoted}");
    if close_on_success {
        format!(
            "{line}; term_status=$?; {}; [ \"$term_status\" -eq 0 ] && exit",
            report("\"$term_status\"")
        )
    } else {
        format!("{line}; {}", report("\"$?\""))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_line_matches_the_shell_family() {
        let path = Path::new("/tmp/command-abc");
        let sentinel = "\\033]2;goddard-command-exit:%s\\033\\\\";
        assert_eq!(
            source_line(Path::new("/bin/zsh"), path, false),
            format!("cat '/tmp/command-abc'; . '/tmp/command-abc'; printf '{sentinel}' \"$?\"")
        );
        assert_eq!(
            source_line(Path::new("/bin/zsh"), path, true),
            format!(
                "cat '/tmp/command-abc'; . '/tmp/command-abc'; term_status=$?; printf '{sentinel}' \"$term_status\"; [ \"$term_status\" -eq 0 ] && exit"
            )
        );
        assert_eq!(
            source_line(Path::new("/opt/homebrew/bin/fish"), path, false),
            format!(
                "cat '/tmp/command-abc'; source '/tmp/command-abc'; printf '{sentinel}' $status"
            )
        );
        assert_eq!(
            source_line(Path::new("/usr/local/bin/nu"), path, true),
            "open --raw '/tmp/command-abc'; source '/tmp/command-abc'; let term_status = $env.LAST_EXIT_CODE; \
             print --no-newline $\"(char esc)]2;goddard-command-exit:($term_status)(char esc)\\\\\"; \
             if $term_status == 0 { exit }"
        );
        assert_eq!(
            source_line(
                Path::new("C:/Program Files/PowerShell/7/pwsh.exe"),
                path,
                true
            ),
            "Get-Content '/tmp/command-abc'; . '/tmp/command-abc'; $term_ok = $?; [Console]::Write(\"$([char]27)]2;goddard-command-exit:$([int](-not $term_ok))$([char]27)\\\"); if ($term_ok) { exit }"
        );
        assert_eq!(
            source_line(Path::new("C:/Windows/System32/cmd.exe"), path, true),
            "type \"/tmp/command-abc\" & call \"/tmp/command-abc\" && exit"
        );
    }

    #[test]
    fn parse_command_exit_reads_only_sentinel_titles() {
        assert_eq!(parse_command_exit("goddard-command-exit:0"), Some(0));
        assert_eq!(parse_command_exit("goddard-command-exit:127"), Some(127));
        assert_eq!(parse_command_exit("goddard-command-exit:1"), Some(1));
        assert_eq!(parse_command_exit("goddard-command-exit:"), None);
        assert_eq!(parse_command_exit("goddard-command-exit:abc"), None);
        assert_eq!(parse_command_exit("vim — ~/project"), None);
    }

    #[test]
    fn source_line_escapes_single_quotes() {
        let path = Path::new("/tmp/it's/command");
        assert_eq!(
            source_line(Path::new("/bin/sh"), path, false),
            r#"cat '/tmp/it'\''s/command'; . '/tmp/it'\''s/command'; printf '\033]2;goddard-command-exit:%s\033\\' "$?""#
        );
    }
}
