//! Deterministic permission exemption for the scoped CLI's read surface.
//! Classify the executable and complete shell input, never display copy.

use serde_json::Value;

/// Inspect a provider's executable input, not its title or justification.
/// Unknown shapes retain the provider's ordinary approval policy.
pub fn readonly_agent_input(input: &Value) -> bool {
    match input {
        Value::String(command) => readonly_agent_command(command),
        Value::Object(_) => {
            for key in ["command", "commandLine"] {
                if let Some(command) = input.get(key) {
                    return command.as_str().is_some_and(readonly_agent_command);
                }
            }
            for key in ["input", "rawInput", "arguments", "rawArgs"] {
                if let Some(value) = input.get(key) {
                    if let Some(encoded) = value.as_str()
                        && let Ok(decoded) = serde_json::from_str::<Value>(encoded)
                    {
                        return readonly_agent_input(&decoded);
                    }
                    return readonly_agent_input(value);
                }
            }
            false
        }
        _ => false,
    }
}

/// Accept simple invocations and sequences consisting entirely of read-only
/// CLI calls. Substitutions, redirects, pipelines and shell wrappers need the
/// normal approval path because they can introduce additional effects.
pub fn readonly_agent_command(command: &str) -> bool {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut started = false;
    let mut quote = None;
    let mut completed = false;
    let mut chars = command.chars().peekable();
    while let Some(c) = chars.next() {
        if quote == Some('\'') {
            if c == '\'' {
                quote = None;
            } else {
                word.push(c);
            }
            continue;
        }
        match c {
            '$' | '`' => return false,
            '\\' => {
                let Some(next) = chars.next() else {
                    return false;
                };
                if next != '\n' {
                    word.push(next);
                    started = true;
                }
            }
            '"' | '\'' if quote.is_none() => {
                quote = Some(c);
                started = true;
            }
            '"' if quote == Some('"') => quote = None,
            _ if quote.is_some() => word.push(c),
            ';' | '\n' | '&' => {
                if c == '&' && chars.next() != Some('&') {
                    return false;
                }
                if started {
                    words.push(std::mem::take(&mut word));
                    started = false;
                }
                if words.is_empty() {
                    if c == '\n' {
                        continue;
                    }
                    return false;
                }
                if !readonly_argv(&words) {
                    return false;
                }
                words.clear();
                completed = c != '&';
            }
            '|' | '<' | '>' | '(' | ')' | '{' | '}' | '*' | '?' | '[' | ']' | '#' | '~' => {
                return false;
            }
            c if c.is_whitespace() => {
                if started {
                    words.push(std::mem::take(&mut word));
                    started = false;
                }
            }
            _ => {
                word.push(c);
                started = true;
            }
        }
    }
    if quote.is_some() {
        return false;
    }
    if started {
        words.push(word);
    }
    if words.is_empty() {
        completed
    } else {
        readonly_argv(&words)
    }
}

fn readonly_argv(words: &[String]) -> bool {
    let Some(executable) = words.first() else {
        return false;
    };
    if executable != "goddard-agent"
        && !(executable.starts_with('/') && executable.ends_with("/goddard-agent"))
    {
        return false;
    }
    let args = &words[1..];
    // Only selectors before any option/content value count as documentation.
    if let Some(index) = args.iter().position(|arg| arg.starts_with('-')) {
        if matches!(args[index].as_str(), "--help" | "-h" | "--schema") {
            return true;
        }
    }
    let path: Vec<_> = args.iter().map(String::as_str).collect();
    if let ["boss", "chain", "--json", payload] = path.as_slice() {
        return matches!(
            serde_json::from_str::<waku_protocol::chains::ChainOperation>(payload),
            Ok(waku_protocol::chains::ChainOperation::Show { .. })
        );
    }
    matches!(
        path.as_slice(),
        [] | ["help" | "schema" | "read" | "search" | "map" | "models", ..]
            | ["history", "search", ..]
            | ["resource", "status", ..]
            | ["command", "list", ..]
            | ["wireframe", "comment", "list" | "read", ..]
            | ["memory", "buckets" | "overview" | "scan" | "zoom", ..]
            | ["history" | "resource" | "command" | "computer" | "merge" | "memory"]
            | ["boss"]
            | [
                "boss",
                "projects"
                    | "instructions"
                    | "persona"
                    | "file"
                    | "memory"
                    | "automation"
                    | "resource-policy"
                    | "employee"
                    | "plan"
                    | "deliverable"
                    | "avatar"
            ]
            | ["boss", "view" | "context" | "roster" | "transcript", ..]
            | ["boss", "projects", "list" | "resolve", ..]
            | ["boss", "instructions", "list" | "show", ..]
            | ["boss", "persona", "list" | "show" | "defaults", ..]
            | ["boss", "file", "list" | "read", ..]
            | [
                "boss",
                "memory",
                "buckets" | "overview" | "scan" | "zoom",
                ..
            ]
            | ["boss", "automation", "list", ..]
            | ["boss", "resource-policy", "show", ..]
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn read_surface_skips_approval() {
        for command in [
            "goddard-agent read task --turn 3",
            "goddard-agent search 'retry logic'",
            "goddard-agent history search --text 'old work'",
            "goddard-agent map --text 'callers?'",
            "goddard-agent boss view outcomes",
            "goddard-agent boss projects resolve --name goddard",
            "goddard-agent memory scan 'auth'",
            "goddard-agent resource status",
            "goddard-agent command list",
            "goddard-agent boss employee --help",
            "'/Applications/Goddard Debug.app/goddard-agent' schema --all",
            "goddard-agent read a && goddard-agent boss roster",
            "goddard-agent read a;\ngoddard-agent resource status\n",
            "goddard-agent read a;",
            "goddard-agent boss chain --json '{\"type\":\"show\",\"chainId\":\"00000000-0000-0000-0000-000000000001\"}'",
        ] {
            assert!(readonly_agent_command(command), "{command}");
        }
        assert!(readonly_agent_input(
            &json!({"input":{"command":"goddard-agent read"}})
        ));
        assert!(readonly_agent_input(
            &json!({"rawArgs":"{\"command\":\"goddard-agent read\"}"})
        ));
    }

    #[test]
    fn writes_and_shell_effects_keep_approval() {
        for command in [
            "goddard-agent rename title",
            "goddard-agent boss chain --json '{\"type\":\"control\"}'",
            "goddard-agent create --text hello",
            "goddard-agent memory summary --json '{}'",
            "goddard-agent boss script --text 'view()'",
            "goddard-agent command upsert --name test",
            "goddard-agent computer js --text foo",
            "goddard-agent resource run -- echo hi",
            "goddard-agent boss persona reset employee",
            "goddard-agent prompt task --text --help",
            "goddard-agent read a; touch /tmp/pwn",
            "goddard-agent read a && goddard-agent archive b",
            "goddard-agent read > file",
            "goddard-agent read $(touch /tmp/pwn)",
            "goddard-agent read \"`touch /tmp/pwn`\"",
            "goddard-agent read | sh",
            "echo goddard-agent read",
            "sh -c 'goddard-agent read'",
            "goddard-agent read &",
            "goddard-agent read 'unterminated",
            "",
        ] {
            assert!(!readonly_agent_command(command), "{command}");
        }
        assert!(!readonly_agent_input(
            &json!({"description":"goddard-agent read"})
        ));
    }
}
