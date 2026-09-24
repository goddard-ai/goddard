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
//! The roster is fixed — `goddard-explore` plus one agent per task class —
//! and class agents resolve their model (and optional effort) through the
//! user's routing class map for the session's provider, so routing and
//! subagents share one user-editable model map.

use std::path::{Path, PathBuf};

use anyhow::Context;
use serde_json::{Value, json};
use waku_protocol::model::{ProviderKind, SubagentDef, SubagentSpec};
use waku_protocol::routing::{RouteClassMap, TaskClass};

/// Agent names carrying this prefix are Goddard-defined; the transcript's
/// background-work rows attribute their runs to us with no extra plumbing.
pub(crate) const NAME_PREFIX: &str = "goddard-";

/// How much of the Goddard subagent contract the provider can enforce at
/// launch. This stays in the daemon because it describes the provider driver,
/// not user settings or persisted session state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SupportLevel {
    /// The provider accepts Goddard's definitions and can deny write tools on
    /// read-only helpers.
    Supported,
    /// The provider can receive routing guidance, but cannot guarantee the
    /// full model/effort or read-only contract per helper invocation.
    Advisory,
    /// The current driver has no Goddard subagent injection path.
    Unsupported,
}

pub(crate) fn support_for(provider: ProviderKind) -> SupportLevel {
    match provider {
        ProviderKind::Claude
        | ProviderKind::Copilot
        | ProviderKind::OpenCode
        | ProviderKind::Pi => SupportLevel::Supported,
        ProviderKind::Codex | ProviderKind::OpenCode2 => SupportLevel::Advisory,
        ProviderKind::Antigravity
        | ProviderKind::Amp
        | ProviderKind::Cursor
        | ProviderKind::DeepSeek
        | ProviderKind::Devin
        | ProviderKind::Droid
        | ProviderKind::Fx
        | ProviderKind::Goose
        | ProviderKind::Grok
        | ProviderKind::Kimi
        | ProviderKind::Muse
        | ProviderKind::OhMyPi => SupportLevel::Unsupported,
    }
}

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

/// The prompt/description baseline for each class's agent. The roster is
/// fixed, so these are the only personalities an agent can carry.
fn class_baseline(class: TaskClass) -> (String, String, bool) {
    match class {
        TaskClass::Routine => (
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
        TaskClass::General => (
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
        TaskClass::Demanding => (
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
/// `goddard-fast`, `goddard-default`, and `goddard-heavy`. Each class agent
/// resolves its model and effort through the session provider's own class
/// map first, then the global routing class map — an entry applies only
/// when it names this session's provider, so a class mapped elsewhere keeps
/// the provider's default model.
pub(crate) fn spec_for(
    provider: ProviderKind,
    provider_classes: Option<&RouteClassMap>,
    classes: &RouteClassMap,
) -> SubagentSpec {
    let mut agents = vec![default_explore()];
    for (class, slug) in [
        (TaskClass::Routine, "fast"),
        (TaskClass::General, "default"),
        (TaskClass::Demanding, "heavy"),
    ] {
        let (description, prompt, read_only) = class_baseline(class);
        let target = provider_classes
            .and_then(|map| map.get(&class))
            .or_else(|| {
                classes
                    .get(&class)
                    .filter(|entry| entry.provider == provider)
            });
        agents.push(SubagentDef {
            name: format!("{NAME_PREFIX}{slug}"),
            description,
            prompt,
            read_only,
            model: target.and_then(|entry| entry.model.clone()),
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
/// the user's config files. The definitions carry the session's resolved
/// model/variant, so the driver must start a private server for this config.
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
                def["tools"] = json!({
                    "bash": false,
                    "write": false,
                    "edit": false,
                    "patch": false
                });
                def["permission"] = json!({
                    "bash": "deny",
                    "edit": "deny",
                    "patch": "deny"
                });
            }
            if let Some(model) = &agent.model {
                def["model"] = json!(model);
            }
            if let Some(effort) = &agent.effort {
                // OpenCode calls the model's reasoning-effort selection a
                // variant, and accepts it on an agent definition as well as
                // on an individual task call.
                def["variant"] = json!(effort);
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
         lookups answerable in one or two tool calls are not worth delegating. \
         OpenCode 2 cannot receive Goddard's per-tier model, effort, or \
         read-only policy over its adopted service, so treat these names as \
         advisory and use the configured provider-native agent when available."
    )
}

/// Copilot's `custom_agents` session config: one `CustomAgentConfig` per spec
/// entry. Per-agent model/effort map straight through. Copilot's built-in
/// read-only tools have stable names, so the allowlist is a hard boundary for
/// the explorer tiers rather than a prompt-only instruction.
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
                if agent.read_only {
                    config.tools = Some(
                        ["view", "grep", "glob", "web_search", "web_fetch"]
                            .into_iter()
                            .map(str::to_owned)
                            .collect(),
                    );
                }
                config
            })
            .collect(),
    )
}

/// Codex registers no agent definitions — its hint names the built-in
/// `spawn_agent` roles and carries the session's resolved tier settings into
/// each call. The native explorer role is still advisory: Goddard cannot
/// install a per-spawn tool policy through the app-server API.
pub(crate) fn codex_hint(spec: &SubagentSpec) -> Option<String> {
    if spec.agents.is_empty() {
        return None;
    }
    let agents = spec
        .agents
        .iter()
        .map(|agent| {
            let role = if agent.read_only {
                "explorer"
            } else {
                "worker"
            };
            let mut settings = vec![format!("agent_type: `{role}`")];
            if let Some(model) = &agent.model {
                settings.push(format!("model: `{model}`"));
            }
            if let Some(effort) = &agent.effort {
                settings.push(format!("reasoning_effort: `{effort}`"));
            }
            format!(
                "- `{}` — {} ({})",
                agent.name,
                agent.description,
                settings.join(", ")
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    Some(format!(
        "This session can delegate focused, self-contained subtasks to helper \
         agents via `spawn_agent` instead of doing everything inline. Use the \
         matching built-in role and pass the listed model/reasoning_effort \
         fields on the call when present:\n{agents}\nThe Codex \
         `explorer` role is advisory for read-only policy because Goddard \
         cannot install a per-spawn tool allowlist. The agent's reply returns \
         into this turn. Trivial lookups answerable in one or two tool calls \
         are not worth delegating."
    ))
}

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
  if (agent.read_only) args.push("--exclude-tools", "edit,write,bash");
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
    use waku_protocol::routing::RouteClassTarget;

    fn classes(pairs: &[(TaskClass, RouteClassTarget)]) -> RouteClassMap {
        pairs.iter().cloned().collect()
    }

    fn entry(
        provider: ProviderKind,
        model: Option<&str>,
        effort: Option<&str>,
    ) -> RouteClassTarget {
        RouteClassTarget {
            provider,
            model: model.map(str::to_owned),
            effort: effort.map(str::to_owned),
        }
    }

    #[test]
    fn default_agents_carry_the_attribution_prefix() {
        let spec = spec_for(ProviderKind::Claude, None, &RouteClassMap::new());
        assert!(!spec.agents.is_empty());
        assert!(
            spec.agents
                .iter()
                .all(|agent| agent.name.starts_with(NAME_PREFIX))
        );
    }

    #[test]
    fn routing_hint_names_every_agent() {
        let spec = spec_for(ProviderKind::Claude, None, &RouteClassMap::new());
        let hint = routing_hint(&spec).expect("a populated spec yields a hint");
        for agent in &spec.agents {
            assert!(hint.contains(&agent.name));
        }
        assert!(routing_hint(&SubagentSpec::default()).is_none());
    }

    #[test]
    fn claude_definitions_mark_read_only_agents() {
        let json = claude_agents_json(&spec_for(ProviderKind::Claude, None, &RouteClassMap::new()))
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
        let classes = classes(&[(
            TaskClass::Routine,
            entry(ProviderKind::OpenCode, Some("openai/gpt-5"), Some("high")),
        )]);
        let json = opencode_config_json(&spec_for(ProviderKind::OpenCode, None, &classes))
            .expect("config serializes");
        let value: Value = serde_json::from_str(&json).unwrap();
        let explore = &value["agent"]["goddard-explore"];
        assert_eq!(explore["mode"], "subagent");
        assert_eq!(explore["permission"]["edit"], "deny");
        assert_eq!(explore["permission"]["bash"], "deny");
        assert_eq!(explore["tools"]["bash"], false);
        assert_eq!(value["agent"]["goddard-fast"]["model"], "openai/gpt-5");
        assert_eq!(value["agent"]["goddard-fast"]["variant"], "high");
    }

    #[test]
    fn class_agents_resolve_models_through_the_class_map() {
        let classes = classes(&[
            (
                TaskClass::Routine,
                entry(ProviderKind::Claude, Some("claude-haiku-4-5"), Some("low")),
            ),
            (
                TaskClass::Demanding,
                entry(ProviderKind::Claude, Some("claude-opus-5"), None),
            ),
        ]);
        let spec = spec_for(ProviderKind::Claude, None, &classes);
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
            "an unmapped class keeps the provider default"
        );
        assert_eq!(spec.agents[3].model.as_deref(), Some("claude-opus-5"));
        assert!(!spec.agents[3].read_only);

        // A class entry naming another provider applies nowhere on this one.
        let codex = spec_for(ProviderKind::Codex, None, &classes);
        assert!(codex.agents.iter().all(|agent| agent.model.is_none()));
    }

    #[test]
    fn per_provider_class_map_wins_over_the_global_map() {
        let provider_classes = classes(&[
            (
                TaskClass::Routine,
                entry(ProviderKind::Claude, Some("claude-haiku-4-5"), Some("low")),
            ),
            (
                TaskClass::General,
                entry(ProviderKind::Claude, Some("claude-sonnet-5"), None),
            ),
        ]);
        let global = classes(&[
            (
                TaskClass::Routine,
                entry(ProviderKind::Claude, Some("claude-opus-5"), None),
            ),
            (
                TaskClass::Demanding,
                entry(ProviderKind::Codex, Some("gpt-5.5"), None),
            ),
        ]);
        let spec = spec_for(ProviderKind::Claude, Some(&provider_classes), &global);
        assert_eq!(spec.agents[1].model.as_deref(), Some("claude-haiku-4-5"));
        assert_eq!(spec.agents[2].model.as_deref(), Some("claude-sonnet-5"));
        // No per-provider hard entry and the global one names another
        // provider — the provider default stands.
        assert_eq!(spec.agents[3].model, None);
    }

    #[test]
    fn opencode2_hint_lists_only_what_exists() {
        assert!(opencode2_hint(&[]).contains("No subagent agents are currently configured"));
        let hint = opencode2_hint(&["explore".into()]);
        assert!(hint.contains("`explore`"));
        assert!(hint.contains("advisory"));
    }

    #[test]
    fn copilot_read_only_agents_use_a_native_tool_allowlist() {
        let classes = classes(&[(
            TaskClass::Routine,
            entry(ProviderKind::Copilot, Some("gpt-5"), Some("medium")),
        )]);
        let agents = copilot_custom_agents(&spec_for(ProviderKind::Copilot, None, &classes))
            .expect("agents serialize");
        let explore = agents
            .iter()
            .find(|agent| agent.name == "goddard-explore")
            .unwrap();
        assert_eq!(
            explore.tools.as_deref(),
            Some(
                &[
                    "view".to_owned(),
                    "grep".to_owned(),
                    "glob".to_owned(),
                    "web_search".to_owned(),
                    "web_fetch".to_owned(),
                ][..]
            )
        );
        let fast = agents
            .iter()
            .find(|agent| agent.name == "goddard-fast")
            .unwrap();
        assert_eq!(fast.model.as_deref(), Some("gpt-5"));
        assert_eq!(fast.reasoning_effort.as_deref(), Some("medium"));
    }

    #[test]
    fn codex_hint_carries_per_tier_model_and_effort() {
        let classes = classes(&[(
            TaskClass::Demanding,
            entry(ProviderKind::Codex, Some("gpt-5.6"), Some("high")),
        )]);
        let hint =
            codex_hint(&spec_for(ProviderKind::Codex, None, &classes)).expect("hint exists");
        assert!(hint.contains("`goddard-heavy`"));
        assert!(hint.contains("agent_type: `worker`"));
        assert!(hint.contains("model: `gpt-5.6`"));
        assert!(hint.contains("reasoning_effort: `high`"));
        assert!(hint.contains("advisory for read-only policy"));
        assert!(codex_hint(&SubagentSpec::default()).is_none());
    }

    #[test]
    fn support_level_matches_the_actual_injection_paths() {
        assert_eq!(support_for(ProviderKind::Claude), SupportLevel::Supported);
        assert_eq!(support_for(ProviderKind::OpenCode), SupportLevel::Supported);
        assert_eq!(support_for(ProviderKind::Copilot), SupportLevel::Supported);
        assert_eq!(support_for(ProviderKind::Pi), SupportLevel::Supported);
        assert_eq!(support_for(ProviderKind::Codex), SupportLevel::Advisory);
        assert_eq!(support_for(ProviderKind::OpenCode2), SupportLevel::Advisory);
        assert_eq!(support_for(ProviderKind::OhMyPi), SupportLevel::Unsupported);
        assert_eq!(support_for(ProviderKind::Amp), SupportLevel::Unsupported);
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
