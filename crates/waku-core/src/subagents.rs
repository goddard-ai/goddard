//! Named subagent definitions injected into a session's harness at launch.
//!
//! Goddard cannot dispatch subagents itself — every harness answers that
//! differently — so this module owns only the shared parts: the `waku-`
//! naming convention (which doubles as the UI attribution key on
//! `BackgroundWorkItem.role`), the default agent set, and the text that
//! teaches a session's model when to delegate. Each driver turns the spec
//! into its own launch-time mechanism: CLI flags for Claude, a config env
//! for OpenCode, an extension file for Pi, a session instruction entry for
//! the adopted OpenCode 2 service, and a thread-start hint for Codex.

use std::path::{Path, PathBuf};

use anyhow::Context;
use serde_json::{Value, json};
use waku_protocol::model::{SubagentDef, SubagentSpec};

/// Agent names carrying this prefix are Goddard-defined; the transcript's
/// background-work rows attribute their runs to us with no extra plumbing.
pub(crate) const NAME_PREFIX: &str = "waku-";

/// The fixed agent set shipped before tiered routing exists: one read-only
/// explorer the session's model can hand lookups to.
pub(crate) fn default_spec() -> SubagentSpec {
    SubagentSpec {
        agents: vec![SubagentDef {
            name: format!("{NAME_PREFIX}explore"),
            description: "Read-only codebase exploration: search, read, and \
                 answer questions about how the code works. Delegate focused \
                 lookups here instead of spending your own context on them."
                .into(),
            prompt: "You are a read-only exploration specialist inside a coding \
                 session. Search, read, and answer questions about the codebase; \
                 never write or modify files. Prefer a single focused pass and \
                 stop as soon as you can answer exactly what was asked. Return \
                 findings concisely: file:line references, quoted snippets, and \
                 a one-line summary. If the task needs edits, say so and return."
                .into(),
            read_only: true,
            model: None,
            effort: None,
        }],
    }
}

/// The routing hint for harnesses that take injected agent definitions: a
/// short system-prompt appendix telling the session's model what it can
/// delegate and to whom.
pub(crate) fn routing_hint(spec: &SubagentSpec) -> Option<String> {
    if spec.agents.is_empty() {
        return None;
    }
    let agents = spec
        .agents
        .iter()
        .map(|agent| format!("- `{}` — {}", agent.name, agent.description))
        .collect::<Vec<_>>()
        .join("\n");
    Some(format!(
        "This session can delegate focused, self-contained subtasks to helper \
         agents instead of doing everything inline:\n{agents}\nInvoke your \
         task/subagent tool with the agent's name; the helper's reply returns \
         into this turn. Trivial lookups you can answer in one or two tool \
         calls are not worth delegating."
    ))
}

/// Claude's `--agents` flag payload: one JSON object mapping agent name to
/// its definition.
pub(crate) fn claude_agents_json(spec: &SubagentSpec) -> Option<String> {
    if spec.agents.is_empty() {
        return None;
    }
    let agents = spec
        .agents
        .iter()
        .map(|agent| {
            let mut def = json!({
                "description": agent.description,
                "prompt": agent.prompt,
            });
            if agent.read_only {
                def["tools"] = json!(["Read", "Grep", "Glob", "LS", "WebFetch", "WebSearch"]);
            }
            if let Some(model) = &agent.model {
                def["model"] = json!(model);
            }
            if let Some(effort) = &agent.effort {
                def["effort"] = json!(effort);
            }
            (agent.name.clone(), def)
        })
        .collect::<serde_json::Map<String, Value>>();
    Some(Value::Object(agents).to_string())
}

/// OpenCode's `OPENCODE_CONFIG_CONTENT` payload: `agent.*` entries merged over
/// the user's config files. Definitions are identical for every session in a
/// workspace, so carrying them on the pooled server is safe.
pub(crate) fn opencode_config_json(spec: &SubagentSpec) -> Option<String> {
    if spec.agents.is_empty() {
        return None;
    }
    let agents = spec
        .agents
        .iter()
        .map(|agent| {
            let mut def = json!({
                "description": agent.description,
                "prompt": agent.prompt,
                "mode": "subagent",
            });
            if agent.read_only {
                def["tools"] = json!({"write": false, "edit": false, "patch": false});
                def["permission"] = json!({"edit": "deny", "patch": "deny"});
            }
            if let Some(model) = &agent.model {
                def["model"] = json!(model);
            }
            (agent.name.clone(), def)
        })
        .collect::<serde_json::Map<String, Value>>();
    Some(json!({"agent": agents}).to_string())
}

/// OpenCode 2's adopted service cannot register agents over its API, so the
/// hint is a session instruction entry naming whatever subagent-mode agents
/// the user's own config already defines.
pub(crate) fn opencode2_hint(available_subagents: &[String]) -> String {
    let roster = if available_subagents.is_empty() {
        "No subagent agents are currently configured.".to_owned()
    } else {
        format!(
            "Available subagent agents: {}.",
            available_subagents
                .iter()
                .map(|name| format!("`{name}`"))
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    format!(
        "This session can delegate focused, self-contained subtasks to helper \
         agents via the `subagent`/`task` tool instead of doing everything \
         inline; the helper's reply returns into this turn. {roster} Trivial \
         lookups answerable in one or two tool calls are not worth delegating."
    )
}

/// Codex registers no agent definitions — its hint names the built-in
/// `spawn_agent` roles instead.
pub(crate) const CODEX_HINT: &str = "This session can delegate focused, \
     self-contained subtasks to helper agents via `spawn_agent` instead of \
     doing everything inline — `explorer` for read-only codebase lookups, \
     `worker` for isolated implementation work. The agent's reply returns \
     into this turn. Trivial lookups answerable in one or two tool calls are \
     not worth delegating.";

/// Pi's delegate tool ships as an extension file written into daemon-owned
/// storage at launch. The tool reads the spec from `WAKU_SUBAGENTS` so the
/// same source serves every tier set.
pub(crate) const PI_EXTENSION_SOURCE: &str = r#"import { spawn } from "node:child_process";
import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { Type } from "typebox";

interface AgentDef {
  name: string;
  description: string;
  prompt: string;
  read_only?: boolean;
  model?: string;
  effort?: string;
}

const spec: { agents: AgentDef[] } = JSON.parse(
  process.env.WAKU_SUBAGENTS ?? '{"agents":[]}',
);
const piBinary = process.env.WAKU_PI_BINARY ?? "pi";

function run(agent: AgentDef, prompt: string): Promise<string> {
  const args = ["-p", "--no-session", "--no-extensions", "--no-skills",
    "--no-context-files", "--system-prompt", agent.prompt];
  if (agent.model) args.push("--model", agent.model);
  if (agent.effort) args.push("--thinking", agent.effort);
  if (agent.read_only) args.push("--exclude-tools", "edit,write");
  args.push("--", prompt);
  return new Promise((resolve, reject) => {
    const child = spawn(piBinary, args, { env: process.env });
    let out = "";
    let err = "";
    child.stdout.on("data", (chunk) => (out += String(chunk)));
    child.stderr.on("data", (chunk) => (err += String(chunk)));
    child.once("error", reject);
    child.once("exit", (code) => {
      if (code === 0) resolve(out.trim());
      else reject(new Error(err.trim() || `pi exited with ${code}`));
    });
  });
}

export default function wakuSubagents(pi: ExtensionAPI) {
  if (!spec.agents.length) return;
  const names = spec.agents.map((agent) => agent.name);
  pi.registerTool({
    name: "waku_delegate",
    label: "Delegate to agent",
    description:
      "Delegate a focused, self-contained subtask to a helper agent and return its reply. The helper shares no context with this session, so the prompt must contain everything it needs. Available agents:\n" +
      spec.agents.map((agent) => `- ${agent.name}: ${agent.description}`).join("\n"),
    parameters: Type.Object(
      {
        agent: Type.Union(names.map((name) => Type.Literal(name)), {
          description: "Which helper agent runs the subtask.",
        }),
        prompt: Type.String({
          minLength: 1,
          description: "Complete instructions for the helper.",
        }),
      },
      { additionalProperties: false },
    ),
    executionMode: "sequential",
    async execute(_id, params) {
      const agent = spec.agents.find((a) => a.name === params.agent);
      if (!agent) throw new Error(`unknown agent ${params.agent}`);
      const text = await run(agent, params.prompt);
      return { content: [{ type: "text", text }], details: {} };
    },
  });
}
"#;

/// Writes the Pi extension under `directory`, which the caller owns.
pub(crate) fn write_pi_extension(directory: &Path) -> anyhow::Result<PathBuf> {
    std::fs::create_dir_all(directory)
        .with_context(|| format!("could not create {}", directory.display()))?;
    let path = directory.join("waku-subagents.ts");
    std::fs::write(&path, PI_EXTENSION_SOURCE)
        .with_context(|| format!("could not write {}", path.display()))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_agents_carry_the_attribution_prefix() {
        let spec = default_spec();
        assert!(!spec.agents.is_empty());
        assert!(
            spec.agents
                .iter()
                .all(|agent| agent.name.starts_with(NAME_PREFIX))
        );
    }

    #[test]
    fn routing_hint_names_every_agent() {
        let spec = default_spec();
        let hint = routing_hint(&spec).expect("a populated spec yields a hint");
        for agent in &spec.agents {
            assert!(hint.contains(&agent.name));
        }
        assert!(routing_hint(&SubagentSpec::default()).is_none());
    }

    #[test]
    fn claude_definitions_mark_read_only_agents() {
        let json = claude_agents_json(&default_spec()).expect("agents serialize");
        let value: Value = serde_json::from_str(&json).unwrap();
        let explore = &value["waku-explore"];
        assert!(!explore["prompt"].as_str().unwrap().is_empty());
        assert!(
            explore["tools"]
                .as_array()
                .unwrap()
                .iter()
                .all(|tool| tool != "Edit" && tool != "Write" && tool != "Bash")
        );
    }

    #[test]
    fn opencode_definitions_are_subagent_mode_and_deny_writes() {
        let json = opencode_config_json(&default_spec()).expect("config serializes");
        let value: Value = serde_json::from_str(&json).unwrap();
        let explore = &value["agent"]["waku-explore"];
        assert_eq!(explore["mode"], "subagent");
        assert_eq!(explore["permission"]["edit"], "deny");
    }

    #[test]
    fn opencode2_hint_lists_only_what_exists() {
        assert!(opencode2_hint(&[]).contains("No subagent agents are currently configured"));
        assert!(opencode2_hint(&["explore".into()]).contains("`explore`"));
    }

    #[test]
    fn pi_extension_round_trips_through_the_filesystem() {
        let directory =
            std::env::temp_dir().join(format!("waku-subagents-test-{}", uuid::Uuid::new_v4()));
        let path = write_pi_extension(&directory).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), PI_EXTENSION_SOURCE);
        let _ = std::fs::remove_dir_all(&directory);
    }
}
