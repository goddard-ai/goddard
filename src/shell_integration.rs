//! Shell integration for PTY terminals.
//!
//! alacritty's vte parser drops the OSC 7 and OSC 133 sequences shells use
//! to report their working directory and command boundaries, so the
//! integration scripts below report them through private OSC 2 titles
//! instead — the one channel that still reaches `TerminalEventProxy` as
//! `Event::Title`. Each report is a sentinel the proxy swallows before it
//! can rename the surface.
//!
//! The hooks install by a `source <script>` line fed as the PTY's first
//! input — the same mechanism custom commands use — which runs after the
//! user's rc files, so every hook appends to or precedes user hooks
//! without replacing them. Shells without a script get no integration and
//! degrade gracefully: no status icon, no live cwd.

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

const BEGIN_TITLE: &str = "waku-shell:begin";
const END_TITLE_PREFIX: &str = "waku-shell:end:";
const CWD_TITLE_PREFIX: &str = "waku-shell:cwd:";

/// A sentinel title's report, if this title is one.
pub fn parse_report(title: &str) -> Option<ShellReport> {
    if title == BEGIN_TITLE {
        return Some(ShellReport::CommandBegin);
    }
    if let Some(code) = title.strip_prefix(END_TITLE_PREFIX) {
        return code.trim().parse().ok().map(ShellReport::CommandEnd);
    }
    title
        .strip_prefix(CWD_TITLE_PREFIX)
        .filter(|cwd| !cwd.is_empty())
        .map(|cwd| ShellReport::Cwd(PathBuf::from(cwd)))
}

// `waku-shell:begin` precedes a command run, `waku-shell:end:<code>`
// follows it, `waku-shell:cwd:<path>` rides every prompt. End sentinels
// only carry weight when a begin marked a run in flight — the scripts
// gate on that themselves, except bash, which cannot (see below), so the
// view ignores an end no begin announced.

const ZSH_SCRIPT: &str = r#"# waku shell integration for zsh.
#
# precmd runs first so it still sees the command's real exit status before
# any user hook can clobber $?, and returns that status so hooks after it
# see it too. preexec marks a run in flight so the first prompt does not
# report a phantom exit.
__waku_running=0
__waku_preexec() {
    __waku_running=1
    builtin printf '\e]2;waku-shell:begin\e\\'
}
__waku_precmd() {
    local __waku_status=$?
    if (( __waku_running )); then
        __waku_running=0
        builtin printf '\e]2;waku-shell:end:%d\e\\' "$__waku_status"
    fi
    builtin printf '\e]2;waku-shell:cwd:%s\e\\' "$PWD"
    return "$__waku_status"
}
precmd_functions=(__waku_precmd ${precmd_functions:#__waku_precmd})
preexec_functions=(__waku_preexec ${preexec_functions:#__waku_preexec})
"#;

const BASH_SCRIPT: &str = r#"# waku shell integration for bash.
#
# PROMPT_COMMAND supplies the post-command report; ours runs first so $?
# is still the command's status, and hands it back to later entries via
# `return`. PS0 supplies the pre-command marker on bash 4.4+; older bash
# ignores the variable, losing the spinner but keeping cwd and status.
__waku_precmd() {
    local __waku_status=$?
    builtin printf '\e]2;waku-shell:end:%d\e\\' "$__waku_status"
    builtin printf '\e]2;waku-shell:cwd:%s\e\\' "$PWD"
    return "$__waku_status"
}
case "$(declare -p PROMPT_COMMAND 2>/dev/null)" in
    "declare -a"*)
        # bash 5.1+ allows PROMPT_COMMAND as an array of entries.
        PROMPT_COMMAND=(__waku_precmd ${PROMPT_COMMAND[@]+"${PROMPT_COMMAND[@]}"})
        ;;
    *)
        PROMPT_COMMAND="__waku_precmd${PROMPT_COMMAND:+;$PROMPT_COMMAND}"
        ;;
esac
PS0='\[\e]2;waku-shell:begin\e\\\]'
"#;

const FISH_SCRIPT: &str = r#"# waku shell integration for fish.
#
# fish_preexec/fish_postexec fire per interactive command; $status inside
# postexec is the command's exit code. The PWD variable watch covers cd.
function __waku_preexec --on-event fish_preexec
    builtin printf '\e]2;waku-shell:begin\e\\'
end
function __waku_postexec --on-event fish_postexec
    builtin printf '\e]2;waku-shell:end:%d\e\\' $status
end
function __waku_pwd --on-variable PWD
    builtin printf '\e]2;waku-shell:cwd:%s\e\\' $PWD
end
builtin printf '\e]2;waku-shell:cwd:%s\e\\' $PWD
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
        Some("zsh") => Some(("waku-integration.zsh", ZSH_SCRIPT)),
        Some("bash") => Some(("waku-integration.bash", BASH_SCRIPT)),
        Some("fish") => Some(("waku-integration.fish", FISH_SCRIPT)),
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

/// The launch line installing a shell's hooks, `source <script>` — read
/// by the interactive shell as its first input, after every rc file. The
/// line echoes once at the first prompt the way a custom command's does.
/// `None` for shells with no integration script.
pub fn launch_line(shell: &Path) -> Option<String> {
    let (name, script) = script_for(shell)?;
    let path = ensure_script(name, script).ok()?;
    Some(format!("source {}", crate::custom_commands::shell_quote(&path)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_parse() {
        assert!(matches!(
            parse_report("waku-shell:begin"),
            Some(ShellReport::CommandBegin)
        ));
        assert!(matches!(
            parse_report("waku-shell:end:0"),
            Some(ShellReport::CommandEnd(0))
        ));
        assert!(matches!(
            parse_report("waku-shell:end: 127"),
            Some(ShellReport::CommandEnd(127))
        ));
        assert!(matches!(
            parse_report("waku-shell:cwd:/tmp/a b"),
            Some(ShellReport::Cwd(ref path)) if path == &PathBuf::from("/tmp/a b")
        ));
        assert!(parse_report("waku-shell:cwd:").is_none());
        assert!(parse_report("a normal title").is_none());
        assert!(parse_report("waku-shell:unknown").is_none());
        // A custom command's exit sentinel stays a title, not a report.
        assert!(parse_report("waku-command-exit:0").is_none());
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
