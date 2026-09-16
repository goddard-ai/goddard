//! Named subagent definitions injected into a session's harness at launch.
//!
//! Goddard cannot dispatch subagents itself — every harness answers that
//! differently — so this module owns only the shared parts: the `goddard-`
//! naming convention (which doubles as the UI attribution key on
//! `BackgroundWorkItem.role`), the default agent set, and the text that
//! teaches a session's model when to delegate. Each driver turns the spec
//! into its own launch-time mechanism: CLI flags for Claude, a config env
//! for OpenCode, an extension file for Pi, a session instruction entry for
//! the adopted OpenCode 2 service, and a thread-start hint for Codex.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::Context;
use serde_json::{Value, json};
use waku_protocol::model::{ProviderKind, SubagentDef, SubagentSpec};
use waku_protocol::settings::SubagentTier;

use crate::usage_history::{RateTable, lookup_rate};

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

/// The prompt/description baseline for a tier name, after the router's
/// @fast/@medium/@heavy split. An unknown name still gets an agent — the
/// user named it — with a generic delegation prompt.
fn tier_baseline(name: &str) -> (String, String, bool) {
    match name {
        "fast" | "explore" => (
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
        "medium" | "implement" | "work" => (
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
        "heavy" | "deep" => (
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
        _ => (
            "A specialized helper agent configured for this session.".into(),
            "You are a specialist inside a coding session. Complete the task \
             exactly as asked and return a concise summary. If the task \
             exceeds your instructions, report back instead of expanding it."
                .into(),
            false,
        ),
    }
}

fn tier_slug(name: &str) -> String {
    name.trim()
        .to_ascii_lowercase()
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '-'
            }
        })
        .collect()
}

/// Relative cost labels baked into each description — "(~5× the cheapest
/// helper's cost)" is the routing signal the session's model actually reads.
/// Labels appear only when two or more agents carry a priced model.
fn annotate_cost(agents: &mut [SubagentDef], rates: &RateTable) {
    let blended = |agent: &SubagentDef| {
        agent
            .model
            .as_deref()
            .and_then(|model| lookup_rate(rates, model))
            .map(|rate| rate.input + rate.output)
    };
    let priced = agents.iter().filter_map(blended).collect::<Vec<_>>();
    if priced.len() < 2 {
        return;
    }
    let cheapest = priced.iter().cloned().fold(f64::INFINITY, f64::min);
    if cheapest <= 0.0 {
        return;
    }
    for agent in agents.iter_mut() {
        let Some(price) = blended(agent) else {
            continue;
        };
        let label = if price / cheapest < 1.5 {
            " (cheapest)".to_owned()
        } else {
            format!(
                " (~{}× the cheapest helper's cost)",
                (price / cheapest).round().max(2.0) as u32
            )
        };
        agent.description.push_str(&label);
    }
}

/// The agent set for one session launch: the built-in `goddard-explore` plus a
/// `goddard-<tier>` agent per configured tier, each carrying the provider's
/// configured model and effort. An `explore` tier customizes the built-in
/// instead of adding a second explorer.
pub(crate) fn spec_for(
    provider: ProviderKind,
    tiers: &BTreeMap<String, SubagentTier>,
    rates: &RateTable,
) -> SubagentSpec {
    let mut agents = vec![default_explore()];
    for (name, tier) in tiers {
        let slug = tier_slug(name);
        if slug.is_empty() {
            continue;
        }
        let target = tier.providers.get(&provider);
        if slug == "explore" {
            if let Some(target) = target {
                agents[0].model = target.model.clone();
                agents[0].effort = target.effort.clone();
            }
            continue;
        }
        let (description, prompt, read_only) = tier_baseline(&slug);
        agents.push(SubagentDef {
            name: format!("{NAME_PREFIX}{slug}"),
            description,
            prompt,
            read_only,
            model: target.and_then(|target| target.model.clone()),
            effort: target.and_then(|target| target.effort.clone()),
        });
    }
    annotate_cost(&mut agents, rates);
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
         into this turn. Prefer the cheapest helper that can finish the task \
         — a task the user tags `budget` favors the cheapest helper that \
         fits, `quality` or `deep` prefers a deeper one. Trivial lookups you \
         can answer in one or two tool calls are not worth delegating."
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
            &BTreeMap::new(),
            &RateTable::unavailable(),
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
            &BTreeMap::new(),
            &RateTable::unavailable(),
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
            &BTreeMap::new(),
            &RateTable::unavailable(),
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
            &BTreeMap::new(),
            &RateTable::unavailable(),
        ))
        .expect("config serializes");
        let value: Value = serde_json::from_str(&json).unwrap();
        let explore = &value["agent"]["goddard-explore"];
        assert_eq!(explore["mode"], "subagent");
        assert_eq!(explore["permission"]["edit"], "deny");
    }

    #[test]
    fn tiers_become_named_agents_with_provider_models() {
        let mut tiers = BTreeMap::new();
        let mut explore = SubagentTier::default();
        explore.providers.insert(
            ProviderKind::Claude,
            waku_protocol::settings::SubagentTierTarget {
                model: Some("claude-haiku-4-5".into()),
                effort: Some("low".into()),
            },
        );
        let mut heavy = SubagentTier::default();
        heavy.providers.insert(
            ProviderKind::Claude,
            waku_protocol::settings::SubagentTierTarget {
                model: Some("claude-opus-4-5".into()),
                effort: None,
            },
        );
        tiers.insert("Explore".into(), explore);
        tiers.insert("heavy".into(), heavy);

        let spec = spec_for(ProviderKind::Claude, &tiers, &RateTable::unavailable());
        assert_eq!(spec.agents.len(), 2, "an explore tier customizes, not adds");
        assert_eq!(spec.agents[0].model.as_deref(), Some("claude-haiku-4-5"));
        assert_eq!(spec.agents[0].effort.as_deref(), Some("low"));
        assert_eq!(spec.agents[1].name, "goddard-heavy");
        assert_eq!(spec.agents[1].model.as_deref(), Some("claude-opus-4-5"));
        assert!(!spec.agents[1].read_only);
    }

    #[test]
    fn priced_models_get_relative_cost_labels() {
        let mut agents = vec![
            SubagentDef {
                name: "goddard-fast".into(),
                description: "cheap".into(),
                prompt: String::new(),
                read_only: false,
                model: Some("claude-haiku-4-5".into()),
                effort: None,
            },
            SubagentDef {
                name: "goddard-heavy".into(),
                description: "deep".into(),
                prompt: String::new(),
                read_only: false,
                model: Some("claude-opus-4-5".into()),
                effort: None,
            },
        ];
        let mut rates = std::collections::HashMap::new();
        rates.insert(
            "claude-haiku-4-5".into(),
            crate::usage_history::ModelRate {
                input: 1.0,
                output: 5.0,
                cache_read: 0.1,
                cache_creation: 1.25,
            },
        );
        rates.insert(
            "claude-opus-4-5".into(),
            crate::usage_history::ModelRate {
                input: 5.0,
                output: 25.0,
                cache_read: 0.5,
                cache_creation: 6.25,
            },
        );
        annotate_cost(
            &mut agents,
            &RateTable {
                rates,
                status: crate::usage_history::PricingStatus::Cached,
            },
        );
        assert!(agents[0].description.ends_with("(cheapest)"));
        assert!(agents[1].description.contains("~5×"));
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
