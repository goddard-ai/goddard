//! Shell integration for PTY terminals.
//!
//! alacritty's vte parser drops the OSC 7 and OSC 133 sequences shells use
//! to report their working directory and command boundaries, so the
//! integration scripts below report them through private OSC 2 titles
//! instead — the one channel that still reaches `TerminalEventProxy` as
//! `Event::Title`. Each report is a sentinel the proxy swallows before it
//! can rename the surface.
//!
//! The hooks install through the user's own startup files: a guarded
//! marker block appended to `.zshrc`/`.bash_profile`/`.bashrc`, or a
//! `conf.d` snippet for fish, so the `source` never echoes at a Goddard
//! prompt the way a typed launch line would. The guard is `$GODDARD`, set
//! only on the PTYs Goddard spawns for plain shells — every other shell on
//! the machine skips the block, and the `-f` test keeps a stale path
//! harmless after an uninstall. Shells without a script get no
//! integration and degrade gracefully: no status icon, no live cwd.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt as _;

/// A report an integration script emits as an OSC 2 sentinel title.
pub enum ShellReport {
    /// The shell accepted a command line and is about to run it.
    CommandBegin,
    /// A command finished; the payload is its exit status.
    CommandEnd(i32),
    /// The shell's working directory — reported at every prompt, which
    /// covers `cd` and subshell returns alike.
    Cwd(PathBuf),
}

/// A sentinel title's report, if this title is one. Scripts materialized by
/// pre-Goddard builds keep emitting `waku-shell:` until their shell restarts,
/// so both prefixes parse.
pub fn parse_report(title: &str) -> Option<ShellReport> {
    let report = title
        .strip_prefix("goddard-shell:")
        .or_else(|| title.strip_prefix("waku-shell:"))?;
    if report == "begin" {
        return Some(ShellReport::CommandBegin);
    }
    if let Some(code) = report.strip_prefix("end:") {
        return code.trim().parse().ok().map(ShellReport::CommandEnd);
    }
    report
        .strip_prefix("cwd:")
        .filter(|cwd| !cwd.is_empty())
        .map(|cwd| ShellReport::Cwd(PathBuf::from(cwd)))
}

// `goddard-shell:begin` precedes a command run, `goddard-shell:end:<code>`
// follows it, `goddard-shell:cwd:<path>` rides every prompt. End sentinels
// only carry weight when a begin marked a run in flight — the scripts
// gate on that themselves, except bash, which cannot (see below), so the
// view ignores an end no begin announced.

const ZSH_SCRIPT: &str = r#"# goddard shell integration for zsh.
#
# precmd runs first so it still sees the command's real exit status before
# any user hook can clobber $?, and returns that status so hooks after it
# see it too. preexec marks a run in flight so the first prompt does not
# report a phantom exit.
(( ${__goddard_loaded:-0} )) && return 0
__goddard_loaded=1
__goddard_running=0
__goddard_preexec() {
    __goddard_running=1
    builtin printf '\e]2;goddard-shell:begin\e\\'
}
__goddard_precmd() {
    local __goddard_status=$?
    if (( __goddard_running )); then
        __goddard_running=0
        builtin printf '\e]2;goddard-shell:end:%d\e\\' "$__goddard_status"
    fi
    builtin printf '\e]2;goddard-shell:cwd:%s\e\\' "$PWD"
    return "$__goddard_status"
}
precmd_functions=(__goddard_precmd ${precmd_functions:#__goddard_precmd})
preexec_functions=(__goddard_preexec ${preexec_functions:#__goddard_preexec})
"#;

const BASH_SCRIPT: &str = r#"# goddard shell integration for bash.
#
# PROMPT_COMMAND supplies the post-command report; ours runs first so $?
# is still the command's status, and hands it back to later entries via
# `return`. PS0 supplies the pre-command marker on bash 4.4+; older bash
# ignores the variable, losing the spinner but keeping cwd and status.
[[ ${__goddard_loaded:-0} -eq 1 ]] && return 0
__goddard_loaded=1
__goddard_precmd() {
    local __goddard_status=$?
    builtin printf '\e]2;goddard-shell:end:%d\e\\' "$__goddard_status"
    builtin printf '\e]2;goddard-shell:cwd:%s\e\\' "$PWD"
    return "$__goddard_status"
}
case "$(declare -p PROMPT_COMMAND 2>/dev/null)" in
    "declare -a"*)
        # bash 5.1+ allows PROMPT_COMMAND as an array of entries.
        PROMPT_COMMAND=(__goddard_precmd ${PROMPT_COMMAND[@]+"${PROMPT_COMMAND[@]}"})
        ;;
    *)
        PROMPT_COMMAND="__goddard_precmd${PROMPT_COMMAND:+;$PROMPT_COMMAND}"
        ;;
esac
PS0='\[\e]2;goddard-shell:begin\e\\\]'
"#;

const FISH_SCRIPT: &str = r#"# goddard shell integration for fish.
#
# fish_preexec/fish_postexec fire per interactive command; $status inside
# postexec is the command's exit code. The PWD variable watch covers cd.
function __goddard_preexec --on-event fish_preexec
    builtin printf '\e]2;goddard-shell:begin\e\\'
end
function __goddard_postexec --on-event fish_postexec
    builtin printf '\e]2;goddard-shell:end:%d\e\\' $status
end
function __goddard_pwd --on-variable PWD
    builtin printf '\e]2;goddard-shell:cwd:%s\e\\' $PWD
end
builtin printf '\e]2;goddard-shell:cwd:%s\e\\' $PWD
"#;

/// The integration script for a shell, keyed by its binary name —
/// `None` for shells with no hook surface (nu, pwsh, cmd).
fn script_for(shell: &Path) -> Option<(&'static str, &'static str)> {
    match shell
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.to_ascii_lowercase())
        .as_deref()
    {
        Some("zsh") => Some(("goddard-integration.zsh", ZSH_SCRIPT)),
        Some("bash") => Some(("goddard-integration.bash", BASH_SCRIPT)),
        Some("fish") => Some(("goddard-integration.fish", FISH_SCRIPT)),
        _ => None,
    }
}

/// Materialize the integration script unless the file already holds it —
/// writes are rare (once per app version's script text) and happen on the
/// PTY spawn path, off the UI thread.
fn ensure_script(name: &str, script: &str) -> io::Result<PathBuf> {
    let path = crate::persistence::shell_integration_scripts_directory().join(name);
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

const BLOCK_BEGIN: &str = "# >>> goddard shell integration >>>";
const BLOCK_END: &str = "# <<< goddard shell integration <<<";
/// Blocks written by pre-Goddard builds; installs strip them so an upgrade
/// swaps the source line in place instead of stacking a second hook.
const LEGACY_BLOCK_BEGIN: &str = "# >>> waku shell integration >>>";
const LEGACY_BLOCK_END: &str = "# <<< waku shell integration <<<";

/// The guarded block appended to a POSIX rc file. `$GODDARD` is set only
/// on terminals Goddard spawns, so every other shell — Terminal.app, SSH,
/// cron — skips it; the `-f` test makes a stale path a no-op rather than
/// an error banner at every prompt.
fn rc_block(script: &Path) -> String {
    let quoted = crate::custom_commands::shell_quote(script);
    format!("{BLOCK_BEGIN}\n[[ -n \"$GODDARD\" && -f {quoted} ]] && source {quoted}\n{BLOCK_END}\n")
}

/// `contents` minus any Goddard or legacy waku block, so installs are
/// idempotent and a moved script path rewrites the block instead of
/// stacking copies.
fn strip_block(contents: &str) -> String {
    let mut kept = String::with_capacity(contents.len());
    let mut in_block = false;
    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed == BLOCK_BEGIN || trimmed == LEGACY_BLOCK_BEGIN {
            in_block = true;
            continue;
        }
        if trimmed == BLOCK_END || trimmed == LEGACY_BLOCK_END {
            in_block = false;
            continue;
        }
        if !in_block {
            kept.push_str(line);
            kept.push('\n');
        }
    }
    kept
}

fn install_rc_block(path: &Path, block: &str) -> io::Result<()> {
    let existing = fs::read_to_string(path).unwrap_or_default();
    let mut updated = strip_block(&existing).trim_end().to_string();
    if !updated.is_empty() {
        updated.push('\n');
    }
    updated.push_str(block);
    if updated == existing {
        return Ok(());
    }
    fs::write(path, updated)
}

/// zsh reads `$ZDOTDIR/.zshrc` when `ZDOTDIR` is set. The app's own
/// environment only carries it for users who export it session-wide —
/// ones set inside `.zshenv` are invisible here and get the plain `~`
/// path, which their zsh then ignores. That misses them; the launch line
/// did not. Accepted: `ZDOTDIR` set in `.zshenv` is rare and the failure
/// is silent degradation, not breakage.
fn zsh_rc_path() -> Option<PathBuf> {
    let directory = std::env::var_os("ZDOTDIR")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(dirs::home_dir)?;
    Some(directory.join(".zshrc"))
}

/// Install the integration so the next spawned shell picks it up
/// silently — no typed `source` line, nothing echoed at the prompt.
/// Returns `true` when the shell has an integration and the caller
/// should export `GODDARD=1` to the PTY so the rc guard passes.
pub fn install(shell: &Path) -> bool {
    let Some((name, script)) = script_for(shell) else {
        return false;
    };
    let Ok(script_path) = ensure_script(name, script) else {
        return false;
    };
    match name {
        "goddard-integration.zsh" => zsh_rc_path()
            .is_some_and(|rc| install_rc_block(&rc, &rc_block(&script_path)).is_ok()),
        "goddard-integration.bash" => dirs::home_dir().is_some_and(|home| {
            // Goddard spawns `bash -l`, which reads `.bash_profile` and
            // skips `.bashrc`; nested interactive shells do the reverse.
            // Both get the block — the script's `__goddard_loaded` guard
            // makes a second source in the same shell a no-op.
            let block = rc_block(&script_path);
            install_rc_block(&home.join(".bash_profile"), &block).is_ok()
                && install_rc_block(&home.join(".bashrc"), &block).is_ok()
        }),
        // conf.d snippets are sourced at every fish startup, so a single
        // file under Goddard's control needs no edit of user-owned config.
        "goddard-integration.fish" => dirs::home_dir().is_some_and(|home| {
            let conf_d = home.join(".config/fish/conf.d");
            // Pre-Goddard installs left a waku-named snippet; drop it so the
            // old script and the new one cannot both hook the shell.
            let legacy = conf_d.join("waku-integration.fish");
            if legacy.is_file() {
                fs::remove_file(&legacy).ok();
            }
            let snippet = conf_d.join("goddard-integration.fish");
            let contents = format!(
                "# goddard shell integration — delete this file to disable.\nstatus is-interactive; and set -q GODDARD; and source {}\n",
                crate::custom_commands::shell_quote(&script_path)
            );
            fs::create_dir_all(snippet.parent().unwrap_or(&home))
                .and_then(|()| {
                    if fs::read(&snippet).is_ok_and(|old| old == contents.as_bytes()) {
                        return Ok(());
                    }
                    fs::write(&snippet, contents)
                })
                .is_ok()
        }),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_parse() {
        assert!(matches!(
            parse_report("goddard-shell:begin"),
            Some(ShellReport::CommandBegin)
        ));
        assert!(matches!(
            parse_report("goddard-shell:end:0"),
            Some(ShellReport::CommandEnd(0))
        ));
        assert!(matches!(
            parse_report("goddard-shell:end: 127"),
            Some(ShellReport::CommandEnd(127))
        ));
        assert!(matches!(
            parse_report("goddard-shell:cwd:/tmp/a b"),
            Some(ShellReport::Cwd(ref path)) if path == &PathBuf::from("/tmp/a b")
        ));
        assert!(matches!(
            parse_report("waku-shell:cwd:/tmp/a b"),
            Some(ShellReport::Cwd(ref path)) if path == &PathBuf::from("/tmp/a b")
        ));
        assert!(parse_report("goddard-shell:cwd:").is_none());
        assert!(parse_report("a normal title").is_none());
        assert!(parse_report("goddard-shell:unknown").is_none());
        // A custom command's exit sentinel stays a title, not a report.
        assert!(parse_report("waku-command-exit:0").is_none());
    }

    #[test]
    fn strip_block_removes_only_the_integration_block() {
        let contents = "export EDITOR=vim\n# >>> goddard shell integration >>>\n[[ -n \"$GODDARD\" ]] && source '/a/b.zsh'\n# <<< goddard shell integration <<<\nalias ll='ls -l'\n";
        assert_eq!(
            strip_block(contents),
            "export EDITOR=vim\nalias ll='ls -l'\n"
        );
        // A pre-Goddard block strips the same way.
        let legacy = "export EDITOR=vim\n# >>> waku shell integration >>>\n[[ -n \"$WAKU\" ]] && source '/a/b.zsh'\n# <<< waku shell integration <<<\nalias ll='ls -l'\n";
        assert_eq!(strip_block(legacy), "export EDITOR=vim\nalias ll='ls -l'\n");
        // An unterminated block eats to EOF; a file without one is untouched.
        assert_eq!(
            strip_block("a\n# >>> goddard shell integration >>>\nb\n"),
            "a\n"
        );
        assert_eq!(strip_block("a\nb\n"), "a\nb\n");
    }

    #[test]
    fn known_shells_get_scripts() {
        assert!(script_for(Path::new("/bin/zsh")).is_some());
        assert!(script_for(Path::new("/bin/bash")).is_some());
        assert!(script_for(Path::new("/opt/homebrew/bin/fish")).is_some());
        assert!(script_for(Path::new("/bin/sh")).is_none());
        assert!(script_for(Path::new("/usr/bin/nu")).is_none());
    }
}
