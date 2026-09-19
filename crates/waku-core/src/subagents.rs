//! Named subagent definitions injected into a session's harness at launch.
//!
//! Goddard cannot dispatch subagents itself — every harness answers that
//! differently — so this module owns only the shared parts: the `goddard-`
//! naming convention (which doubles as the UI attribution key on
//! `BackgroundWorkItem.role`), the fixed agent roster, and the text that
//! teaches a session's model when to delegate. Each driver turns the spec
//! into its own launch-time mechanism: CLI flags for Claude, a config env
//! for OpenCode, an extension file for Pi, a session instruction entry for
//! the adopted OpenCode 2 service, and a thread-start hint for Codex.
//!
//! The roster is fixed — `goddard-explore` plus one agent per routing tier —
//! and tier agents resolve their model (and optional effort) through the
//! route policy's `tiers` table for the session's provider, so routing and
//! subagents share one user-editable model map.

use std::path::{Path, PathBuf};

use anyhow::Context;
use serde_json::{Value, json};
use waku_protocol::model::{ProviderKind, SubagentDef, SubagentSpec};

use crate::route_policy::{RoutePolicy, Tier};

/// Agent names carrying this prefix are Goddard-defined; the transcript's
/// background-work rows attribute their runs to us with no extra plumbing.
pub(crate) const NAME_PREFIX: &str = "goddard-";

fn default_explore() -> SubagentDef {
    SubagentDef {
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
    }
}

/// The prompt/description baseline for each routing tier. The roster is
/// fixed, so these are the only personalities an agent can carry.
fn tier_baseline(tier: Tier) -> (String, String, bool) {
    match tier {
        Tier::Fast => (
            "Read-only lookups, search, and quick questions — the cheap pass \
             for anything answerable without edits."
                .into(),
            "You are a read-only exploration specialist inside a coding \
             session. Search, read, and answer questions about the codebase; \
             never write or modify files. Prefer a single focused pass and \
             stop as soon as you can answer exactly what was asked. Return \
             findings concisely: file:line references, quoted snippets, and \
             a one-line summary. If the task needs edits, say so and return."
                .into(),
            true,
        ),
        Tier::Default => (
            "Focused implementation work: edits, refactoring, and tests. \
             Delegate bounded coding tasks here."
                .into(),
            "You are an implementation specialist inside a coding session. \
             Complete the bounded task exactly as asked — edit, refactor, or \
             add tests — and return a one-line summary of what changed and \
             where. Stay inside the task's scope; if it grows beyond what \
             was asked, report back instead of expanding it."
                .into(),
            false,
        ),
        Tier::Heavy => (
            "Deep analysis: architecture, debugging, and security review. \
             Reserve this for the hardest problems."
                .into(),
            "You are a deep-analysis specialist inside a coding session. \
             Reason carefully about the problem — architecture, debugging, \
             or security — and explore before concluding. Return a concise, \
             well-supported answer with file:line evidence."
                .into(),
            false,
        ),
    }
}

/// The agent set for one session launch: the built-in `goddard-explore` plus
/// `goddard-fast`, `goddard-default`, and `goddard-heavy`. Each tier agent
/// resolves its model and effort through the route policy's `tiers` table
/// for the session's provider; a tier the policy leaves unmapped keeps the
/// provider's default model.
pub(crate) fn spec_for(provider: ProviderKind, policy: &RoutePolicy) -> SubagentSpec {
    let mut agents = vec![default_explore()];
    for (tier, slug) in [
        (Tier::Fast, "fast"),
        (Tier::Default, "default"),
        (Tier::Heavy, "heavy"),
    ] {
        let (description, prompt, read_only) = tier_baseline(tier);
        let target = policy
            .tiers
            .get(&provider)
            .and_then(|table| table.get(&tier));
        agents.push(SubagentDef {
            name: format!("{NAME_PREFIX}{slug}"),
            description,
            prompt,
            read_only,
            model: target.map(|entry| entry.model.clone()),
            effort: target.and_then(|entry| entry.effort.clone()),
        });
    }
    SubagentSpec { agents }
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
         into this turn. Prefer the lightest tier that can finish the task \
         — a task the user tags `budget` favors `goddard-fast`, `quality` or \
         `deep` prefers `goddard-heavy`. Trivial lookups you can answer in \
         one or two tool calls are not worth delegating."
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

/// Copilot's `custom_agents` session config: one `CustomAgentConfig` per spec
/// entry. Per-agent model/effort map straight through. `read_only` is not
/// expressed — the tool allowlist takes exact CLI tool names, and an
/// unverifiable guess would strip the agent of working tools entirely.
pub(crate) fn copilot_custom_agents(
    spec: &SubagentSpec,
) -> Option<Vec<github_copilot_sdk::types::CustomAgentConfig>> {
    if spec.agents.is_empty() {
        return None;
    }
    Some(
        spec.agents
            .iter()
            .map(|agent| {
                let mut config = github_copilot_sdk::types::CustomAgentConfig::new(
                    agent.name.clone(),
                    agent.prompt.clone(),
                );
                config.display_name = Some(agent.name.clone());
                config.description = Some(agent.description.clone());
                config.infer = Some(true);
                config.model = agent.model.clone();
                config.reasoning_effort = agent.effort.clone();
                config
            })
            .collect(),
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
/// storage at launch. The tool reads the spec from `GODDARD_SUBAGENTS` so the
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
  process.env.GODDARD_SUBAGENTS ?? '{"agents":[]}',
);
const piBinary = process.env.GODDARD_PI_BINARY ?? "pi";

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

export default function goddardSubagents(pi: ExtensionAPI) {
  if (!spec.agents.length) return;
  const names = spec.agents.map((agent) => agent.name);
  pi.registerTool({
    name: "goddard_delegate",
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
    let path = directory.join("goddard-subagents.ts");
    std::fs::write(&path, PI_EXTENSION_SOURCE)
        .with_context(|| format!("could not write {}", path.display()))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_agents_carry_the_attribution_prefix() {
        let spec = spec_for(
            ProviderKind::Claude,
            &crate::route_policy::shipped_default_policy(),
        );
        assert!(!spec.agents.is_empty());
        assert!(
            spec.agents
                .iter()
                .all(|agent| agent.name.starts_with(NAME_PREFIX))
        );
    }

    #[test]
    fn routing_hint_names_every_agent() {
        let spec = spec_for(
            ProviderKind::Claude,
            &crate::route_policy::shipped_default_policy(),
        );
        let hint = routing_hint(&spec).expect("a populated spec yields a hint");
        for agent in &spec.agents {
            assert!(hint.contains(&agent.name));
        }
        assert!(routing_hint(&SubagentSpec::default()).is_none());
    }

    #[test]
    fn claude_definitions_mark_read_only_agents() {
        let json = claude_agents_json(&spec_for(
            ProviderKind::Claude,
            &crate::route_policy::shipped_default_policy(),
        ))
        .expect("agents serialize");
        let value: Value = serde_json::from_str(&json).unwrap();
        let explore = &value["goddard-explore"];
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
        let json = opencode_config_json(&spec_for(
            ProviderKind::Claude,
            &crate::route_policy::shipped_default_policy(),
        ))
        .expect("config serializes");
        let value: Value = serde_json::from_str(&json).unwrap();
        let explore = &value["agent"]["goddard-explore"];
        assert_eq!(explore["mode"], "subagent");
        assert_eq!(explore["permission"]["edit"], "deny");
    }

    #[test]
    fn tier_agents_resolve_models_through_the_route_policy() {
        let policy = crate::route_policy::parse_policy(
            r#"{"version": 1,
                "classes": {"routine": "tier:fast", "general": "tier:default", "demanding": "tier:heavy"},
                "tiers": {"claude": {
                    "fast": {"model": "claude-haiku-4-5", "effort": "low"},
                    "heavy": "claude-opus-5"
                }}}"#,
        )
        .unwrap();
        let spec = spec_for(ProviderKind::Claude, &policy);
        let names: Vec<&str> = spec
            .agents
            .iter()
            .map(|agent| agent.name.as_str())
            .collect();
        assert_eq!(
            names,
            [
                "goddard-explore",
                "goddard-fast",
                "goddard-default",
                "goddard-heavy"
            ]
        );
        assert_eq!(spec.agents[1].model.as_deref(), Some("claude-haiku-4-5"));
        assert_eq!(spec.agents[1].effort.as_deref(), Some("low"));
        assert!(spec.agents[1].read_only);
        assert_eq!(
            spec.agents[2].model, None,
            "an unmapped tier keeps the provider default"
        );
        assert_eq!(spec.agents[3].model.as_deref(), Some("claude-opus-5"));
        assert!(!spec.agents[3].read_only);

        // A provider with no tiers entry leaves every tier agent unmapped.
        let codex = spec_for(ProviderKind::Codex, &policy);
        assert!(codex.agents.iter().all(|agent| agent.model.is_none()));
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
