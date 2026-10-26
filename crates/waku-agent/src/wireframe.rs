//! Wireframe preview and durable discussion commands.

use anyhow::{Result, anyhow, bail};
use waku_protocol::{Command, wireframe_comments::WireframeCommentOperation as Op};

pub(super) fn command(args: Vec<String>) -> Result<Command> {
    if args.first().is_none_or(|arg| arg != "comment") {
        let (pos, _) = super::flags(args, &[], true)?;
        if pos.len() != 1 {
            bail!("usage: goddard-agent wireframe PATH.wireframe.html|PATH.wireframe.json");
        }
        return Ok(Command::AgentWireframe {
            path: pos[0].clone(),
        });
    }
    let action = args
        .get(1)
        .ok_or_else(|| anyhow!("use wireframe comment add|list|read|reply; see --help"))?;
    let allowed: &[&str] = match action.as_str() {
        "add" => &["screen", "anchor", "text", "file", "idempotency-key"],
        "reply" => &["thread", "text", "file", "idempotency-key"],
        "list" => &["offset"],
        "read" => &["thread"],
        _ => bail!("unknown wireframe comment operation; use add, list, read, or reply"),
    };
    let (pos, values) = super::flags(args[2..].to_vec(), allowed, true)?;
    if pos.len() != 1 {
        bail!("provide exactly one workspace-relative HTML wireframe path");
    }
    let required = |key: &str| {
        values
            .get(key)
            .cloned()
            .ok_or_else(|| anyhow!("--{key} is required"))
    };
    let operation = match action.as_str() {
        "add" => Op::Add {
            screen: required("screen")?,
            anchor: values.get("anchor").cloned(),
            body: super::raw_input(&values)?,
            key: required("idempotency-key")?,
        },
        "reply" => Op::Reply {
            thread: required("thread")?.parse()?,
            body: super::raw_input(&values)?,
            key: required("idempotency-key")?,
        },
        "read" => Op::Get {
            thread: required("thread")?.parse()?,
        },
        _ => Op::List {
            offset: values
                .get("offset")
                .map(|value| value.parse())
                .transpose()?
                .unwrap_or(0),
        },
    };
    Ok(Command::WireframeComments {
        path: pos[0].clone(),
        document: None,
        revision: None,
        operation,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn args(input: &[&str]) -> Vec<String> {
        input.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn comment_commands_keep_plain_text_and_retry_identity() {
        let result = command(args(&[
            "comment",
            "add",
            "design.wireframe.html",
            "--screen",
            "tasks",
            "--anchor",
            "sidebar",
            "--text",
            "Keep\nthis visible",
            "--idempotency-key",
            "sidebar-v1",
        ]))
        .unwrap();
        match result {
            Command::WireframeComments {
                path,
                operation: Op::Add { body, key, .. },
                ..
            } => {
                assert_eq!(path, "design.wireframe.html");
                assert_eq!(body, "Keep\nthis visible");
                assert_eq!(key, "sidebar-v1");
            }
            _ => panic!("expected comment creation"),
        }
        assert!(
            command(args(&[
                "comment",
                "reply",
                "design.wireframe.html",
                "--thread",
                "invalid",
                "--text",
                "reply",
                "--idempotency-key",
                "reply-v1"
            ]))
            .is_err()
        );
        assert!(
            command(args(&[
                "comment",
                "add",
                "design.wireframe.html",
                "--screen",
                "tasks",
                "--text",
                "missing retry identity"
            ]))
            .is_err()
        );
    }
}
