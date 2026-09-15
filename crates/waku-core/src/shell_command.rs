//! Daemon-owned one-shot shell-command generation for a terminal's command
//! bar. Same machinery as commit-message generation; every entry point
//! performs process I/O and must run on the background executor.

use std::path::Path;

use anyhow::{anyhow, bail};

use crate::git_commit::{AgentInvocation, agent_oneshot, git_optional_stdout, strip_ansi};

/// The client already bounds scrollback before sending; this cap is the
/// prompt's own guard against an unbounded caller.
const MAX_SCROLLBACK_BYTES: usize = 12 * 1024;
const MAX_GIT_STATUS_LINES: usize = 30;
const MAX_COMMAND_CHARS: usize = 4_000;

pub fn generate_command(
    cwd: &Path,
    request: &str,
    scrollback: Option<&str>,
    shell: Option<&str>,
    invocation: &AgentInvocation,
) -> anyhow::Result<String> {
    let request = request.trim();
    if request.is_empty() {
        bail!("describe the command to generate");
    }
    let prompt = command_prompt(cwd, request, scrollback, shell);
    let output = agent_oneshot(cwd, &prompt, invocation, "a shell command")?;
    normalize_command(&output).ok_or_else(|| {
        anyhow!(
            "{} returned no shell command",
            invocation.provider.display_name()
        )
    })
}

fn command_prompt(
    cwd: &Path,
    request: &str,
    scrollback: Option<&str>,
    shell: Option<&str>,
) -> String {
    let mut context = format!(
        "Shell: {}\nOS: {} {}\nWorking directory: {}\n",
        shell
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .unwrap_or("unknown"),
        std::env::consts::OS,
        std::env::consts::ARCH,
        cwd.display(),
    );
    // `git status` fails outside a repository; an absent section beats an
    // error line the model might echo into the command.
    if let Ok(Some(status)) =
        git_optional_stdout(cwd, &["status", "--short", "--untracked-files=all"])
    {
        let status = status
            .lines()
            .take(MAX_GIT_STATUS_LINES)
            .collect::<Vec<_>>()
            .join("\n");
        if !status.is_empty() {
            context.push_str(&format!("\nGit status:\n{status}\n"));
        }
    }
    if let Some(scrollback) = scrollback.map(str::trim).filter(|text| !text.is_empty()) {
        let scrollback = truncate_bytes(scrollback, MAX_SCROLLBACK_BYTES);
        context.push_str(&format!("\nRecent terminal output:\n{scrollback}\n"));
    }
    format!(
        "Write a shell command for the request below.\n\
         Return only the command text itself: no Markdown fences, no explanation, no leading \"$\". \
         One line when one suffices; more only when the request truly needs them. \
         Do not call tools; all context is included here.\n\n{context}\nRequest: {request}"
    )
}

/// Hard cap at a char boundary, dropping the oldest text — the newest
/// scrollback lines carry the context a request refers to.
fn truncate_bytes(text: &str, limit: usize) -> &str {
    if text.len() <= limit {
        return text;
    }
    let mut start = text.len() - limit;
    while !text.is_char_boundary(start) {
        start += 1;
    }
    &text[start..]
}

/// The contract is one command and nothing else; normalization peels the
/// wrapping models still add — fences, a "Command:" lead-in, a "$" prompt,
/// matching quotes — while interior newlines survive for genuinely
/// multi-line answers.
fn normalize_command(output: &str) -> Option<String> {
    let clean = strip_ansi(output);
    let mut lines: Vec<&str> = clean
        .lines()
        .map(str::trim_end)
        .skip_while(|line| line.trim().is_empty())
        .collect();
    while lines.last().is_some_and(|line| line.trim().is_empty()) {
        lines.pop();
    }
    if lines
        .first()
        .is_some_and(|line| line.trim_start().starts_with("```"))
    {
        lines.remove(0);
    }
    if lines
        .last()
        .is_some_and(|line| line.trim_start().starts_with("```"))
    {
        lines.pop();
    }
    let joined = lines.join("\n");
    let command = joined.trim();
    let command = if command.contains('\n') {
        command.to_owned()
    } else {
        let command = command
            .strip_prefix("Command:")
            .or_else(|| command.strip_prefix("command:"))
            .unwrap_or(command)
            .trim();
        let command = command.strip_prefix("$ ").unwrap_or(command);
        ['`', '"', '\'']
            .iter()
            .find_map(|wrapper| {
                command
                    .strip_prefix(*wrapper)
                    .and_then(|line| line.strip_suffix(*wrapper))
                    .map(str::trim)
            })
            .unwrap_or(command)
            .to_owned()
    };
    if command.is_empty() {
        return None;
    }
    Some(command.chars().take(MAX_COMMAND_CHARS).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_peels_fences_prefixes_and_prompt_marks() {
        assert_eq!(
            normalize_command("```sh\nfind . -type f -size +100M\n```\n").as_deref(),
            Some("find . -type f -size +100M")
        );
        assert_eq!(
            normalize_command("Command: $ ls -la").as_deref(),
            Some("ls -la")
        );
        assert_eq!(
            normalize_command("\"git status\"").as_deref(),
            Some("git status")
        );
    }

    #[test]
    fn normalize_keeps_interior_newlines() {
        assert_eq!(
            normalize_command("cd /tmp\nls -la\n").as_deref(),
            Some("cd /tmp\nls -la")
        );
        // A heredoc's indented lines keep their indentation.
        assert_eq!(
            normalize_command("cat <<EOF\n  hello\nEOF").as_deref(),
            Some("cat <<EOF\n  hello\nEOF")
        );
    }

    #[test]
    fn normalize_rejects_empty_output() {
        assert_eq!(normalize_command(""), None);
        assert_eq!(normalize_command("```\n```"), None);
        assert_eq!(normalize_command("  \n\n"), None);
    }

    #[test]
    fn prompt_embeds_shell_cwd_and_scrollback() {
        let prompt = command_prompt(
            Path::new("/tmp/repo"),
            "list big files",
            Some("$ ls\nfile.txt\n"),
            Some("zsh"),
        );
        assert!(prompt.contains("Shell: zsh"));
        assert!(prompt.contains("Working directory: /tmp/repo"));
        assert!(prompt.contains("file.txt"));
        assert!(prompt.contains("Request: list big files"));
    }

    #[test]
    fn scrollback_cap_drops_the_oldest_bytes() {
        let text = "ab\n".repeat(10_000);
        let kept = truncate_bytes(&text, 1024);
        assert_eq!(kept.len(), 1024);
        assert!(kept.ends_with("ab\n"));
    }
}
