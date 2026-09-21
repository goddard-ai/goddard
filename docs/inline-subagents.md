# Inline subagents

Delegating work *inside* a session through each harness's own subagent
mechanism — Claude Code's `Agent` tool, Codex's `spawn_agent`, OpenCode's
`task`/`subagent` tool — on a model the harness can reach natively. This is
deliberately **not** `goddard-agent create`: that spawns a new top-level Goddard
task with its own transcript, while an inline subagent reports back into the
same turn and renders through the background-work surface we already have.

The goal of this document: the smallest coherent feature that lets a session
route work to model tiers (`fast` explorer, `medium` implementer, `heavy`
reasoner), attribute subagent runs to tiers in the UI, and — where the harness
permits — cap or annotate subagent spend. Evaluated from
[opencode-model-router](https://github.com/marco-jardim/opencode-model-router);
the generalizable concepts are tier routing, per-tier prompts, call caps with
live annotations, and enforcement. What does *not* generalize is the delivery
mechanism — every harness answers that differently, which is what most of this
document is about.

## What exists today

- **Observation is solved; dispatch is not.** Subagent work already normalizes
  into `BackgroundWorkItem` (`BackgroundWorkKind::Subagent`,
  `crates/waku-protocol/src/model.rs:2199`) carried on
  `DriverEvent::BackgroundWork` (`model.rs:2305`). The item already has the
  attribution fields a tier needs: `role`, `model`, `parent_id`,
  `origin_activity_id`, `control_id` (`model.rs:2246`).
- **The write channel is fixed.** `DriverControl` (`driver/mod.rs:153`) covers
  prompt / steer / cancel / respond / `apply_options(SessionOptions)` /
  rollback / fork. `SessionOptions` (`driver/mod.rs:207`) is only
  mode/model/effort/service-tier/context-window — anything else must ride
  `DriverStartOptions` (`driver/mod.rs:185`) or become a new control method.
- **There is already an injection precedent split by transport shape.**
  Per-session processes get `apply_agent_environment` — env vars + a PATH
  prepend (`command_env.rs:57`). Shared services get a session-scoped
  *instruction* instead: `write_session_shim` + `shared_service_instruction`
  (`agent.rs:438`, `agent.rs:528`) delivered via
  `PUT /api/session/{id}/instructions/entries/{key}` for OpenCode 2
  (`opencode2.rs:661`). Inline subagents should reuse exactly this split:
  argv/env for owned processes, session instructions for adopted ones.
- **Codex already proves the shape works.** `thread/start` accepts
  `baseInstructions`/`developerInstructions` — used today for the title
  thread (`codex.rs:1137-1140`) — and `collabAgentToolCall` events already
  yield `model` + `agentType` per spawned agent (`codex.rs:1564`).
- **`agent_preset` is a sibling concept, not this one.** It picks the
  *primary* agent for OpenCode 2 / DeepSeek sessions (`driver/mod.rs:193`,
  `opencode2_session.rs:451`). Subagent tiers are dispatch targets, not
  session compositions — `agent_presets` already filters `mode: subagent`
  entries out (`opencode2_session.rs:453`).

## The tier model

A tier is a named bundle in app settings (home: `PersistedState`/`AppSettings`,
`persistence.rs:194`), resolved per provider at session start:

```rust
struct SubagentTier {
    id: String,            // "fast" | "medium" | "heavy" — becomes agent name `goddard-<id>`
    model: String,         // harness-native model id (NOT a catalog id — see below)
    effort: Option<String>,
    read_only: bool,       // explorer tiers: deny write tools where the harness allows
    call_cap: Option<u32>, // max invocations per session; None = uncapped
    prompt: Option<String>// per-tier instruction override; default provided per harness
}
```

Two non-obvious constraints:

- **`model` must be harness-native.** Dispatch params are harness-local:
  Claude wants `sonnet`/`opus`/`haiku`/full-id/`inherit`, OpenCode wants
  `provider/model-id`, Codex wants its own ids. The catalog's
  `ProviderModel.id` format (`provider/model`, `opencode2_session.rs:431`) is
  right for OpenCode only. Tiers therefore store per-provider model strings;
  the catalog (`ProviderModel.reasoning_efforts`, `model.rs:511`) is where the
  settings UI validates them, not where dispatch reads them.
- **Tier *names* are the attribution key.** Naming every injected agent
  `goddard-<tier>` means `BackgroundWorkItem.role` — already parsed from
  `subagent_type`/`agentType` (`claude.rs:987`, `codex.rs:1588`) — carries the
  tier with zero new wire plumbing.

## Per-harness dispatch

### Claude Code (`driver/claude.rs`)

The driver spawns a dedicated `claude -p --input-format stream-json` process
per session (`claude.rs:139-167`), so **argv is ours**.

- **Subagent defs:** `--agents '<json>'` — a launch flag carrying full agent
  definitions (`name`, `description`, `prompt`, `tools`, `model`,
  `permissionMode`) that outranks `.claude/agents/` files. No writes into the
  user's repo or `~/.claude`. Model per agent: alias or full id; the
  `Agent` tool also accepts a per-call `model` override.
- **Routing hint:** `--append-system-prompt '<text>'` (works in `-p` mode)
  teaches the parent when to prefer `goddard-fast` over the built-in `Explore`.
  Optionally `--append-subagent-system-prompt` for a policy every subagent
  inherits.
- **Tier prompts:** the `prompt` field of the `--agents` JSON.
- **Read-only tiers:** `tools`/`disallowedTools` in the same JSON, or
  `permissionMode` on the agent.
- **Caps/enforcement:** `--settings '<json>'` can register `PreToolUse`
  (deny/`updatedInput`) and `PostToolUse` (`additionalContext`) hooks for this
  session only — file-less, no user-settings edits. A `PostToolUse` hook on
  the `Agent` tool is where `[cap: N/MAX]` annotations live; `PreToolUse`
  deny is the hard cap. Handler can be a goddard-owned script/binary (the
  `goddard-agent` shim pattern, `agent.rs:280`); per-session counting can live
  in the daemon since the token already scopes the session.
- **Observation:** `task_started` events already carry `subagent_type` →
  `role` and `model` (`claude.rs:929-998`); `can_stop`+`control_id` are wired
  to `stop_background_work` (`claude.rs:553`). Missing: nothing structural —
  attribution works if the `model` field is present on the wire event (verify
  against a live stream; it may only be inferable from the `Agent` tool_use
  input).

This is the reference implementation target: everything needed exists via
launch flags on a process we own.

### Codex (`driver/codex.rs`)

`codex app-server --stdio` per session (`codex.rs:251`); Goddard already uses
`-c` config overrides and env for Computer Use (`codex.rs:165-208`) and
`skills/extraRoots/set` for skill injection (`codex.rs:352`).

- **Subagent defs:** native `spawn_agent` tool — v2 accepts `agent_type`,
  `message`, `model`, `reasoning_effort`, `service_tier`, `fork_turns`.
  Built-in roles `default`/`worker`/`explorer`; custom roles are TOML files
  in `~/.codex/agents/` or `.codex/agents/` with `name`, `description`,
  `developer_instructions`, `model`, `model_reasoning_effort`.
- **Routing hint:** `developerInstructions` on `thread/start` — a channel the
  driver already uses for the title thread (`codex.rs:1137-1140`), free for
  the session thread.
- **Two dispatch strategies, in order of preference:**
  1. *Hint-only*: `developerInstructions` tells the model to call
     `spawn_agent` with `agent_type: "explorer"` + explicit `model`/
     `reasoning_effort`. No files anywhere; per-call model is native.
     Caveat: upstream bugs where the role layer drops `model`/effort
     overrides, and `fork_context`-style spawns reject overrides — pin and
     test the exact Codex build.
  2. *Agent files*: write `~/.codex/agents/goddard-{fast,medium,heavy}.toml`
     (additive, namespaced; do **not** redirect `CODEX_HOME` — auth lives
     there). Needed if we want per-tier `developer_instructions` rather than
     relying on the parent to paste prompts into `message`.
- **Caps:** `~/.codex/hooks.json` supports `PreToolUse` deny + `SubagentStart`/
  `SubagentStop` — but it is a *global* user file (would fire for the user's
  own `codex` runs) and `PreToolUse` coverage is incomplete upstream (misses
  `apply_patch`, some MCP). Steer-nudge degrade (`supports_steer`, `codex.rs:
  1000`) is the honest v1.
- **Observation:** already the best — `codex_subagent_work` extracts `model`,
  `agentType`/`role`, `senderThreadId` → `parent_id`, per-agent states
  (`codex.rs:1564-1625`).

### OpenCode 2 (`driver/opencode2.rs`) — the constrained one

The service is *adopted*, not spawned (`opencode2_service.rs:1-30`): one
user-owned background process serves every workspace. **We cannot touch its
env, its config, or its plugin set.** This forecloses `OPENCODE_CONFIG*` env
injection entirely.

- **Subagent defs:** v2's `subagent` tool takes `agent`, `prompt`,
  `sessionID` — **no model parameter**. A child runs the model on its agent
  definition, and agent definitions live in the service's config (the user's).
  Goddard can only *name* agents that already exist.
- **Routing hint:** session instruction entries — the exact channel the
  `goddard-agent` shim already uses (`opencode2.rs:661-688`, API at
  `opencode2_api.rs:945`). A `goddard-subagents` entry can describe a routing
  policy and name whatever `subagent`-mode agents `list_agents` reports
  (`opencode2.rs:444`, `opencode2_api.rs:1248`).
- **Tier models:** only achievable if the user's config defines agents with
  the right models, or if we *create* agents — there is no agent-register
  route in the API surface used today. Degrade: hint names desired agents;
  missing ones fall back to the session model. Alternatively v2 `commands`
  can bind `agent`+`model`+`subagent: true`, but commands are also
  config-defined — same wall.
- **Caps:** no per-session plugin/hook surface reachable over the adopted
  API. `steer()` (`supports_steer: true`, `opencode2.rs:644`) nudge at cap is
  the only enforcement-shaped lever.
- **Observation gap:** opencode2 emits `BackgroundWork` only for detached
  shells (`opencode2.rs:2046-2085`). Subagent runs surface only indirectly —
  `StepState` notes a step "can run a different model than the session (a
  subagent step)" (`opencode2.rs:209-214`). Tier attribution needs new work:
  map `subagent` tool calls / child sessions into `BackgroundWorkItem`s. The
  API exposes session children (`/api/session` lists them,
  `opencode2_session.rs:23-26`) if event-stream mapping proves thin.

### OpenCode v1 (`driver/opencode.rs`)

A pooled `opencode serve` per workspace (`opencode.rs:1-26`,
`opencode_pool.rs`) whose env Goddard controls at acquire — but the server is
shared by every session in the workspace (`opencode.rs:230-242`), so injection
is per-workspace, not per-session. Fine for additive tier definitions.

- **Subagent defs:** `OPENCODE_CONFIG_CONTENT` / `OPENCODE_CONFIG` env on
  server spawn can define `agent.*` entries (`mode: "subagent"`, `model`,
  `prompt`, `permission: {edit: deny, bash: deny}` for read-only tiers).
- **Dispatch:** `task` tool accepts `model` and `variant` per call (gated by
  the `model_override` permission, which our injected config can allow).
- **Caps:** a plugin file in the config dir gets `tool.execute.before/after`
  hooks — can mutate args and append `[cap: N/MAX]` to results.
- **Observation:** no `Subagent` work emission today; same class of follow-up
  as opencode2 (child sessions exist on `/session/:id/children`).

### ACP (`driver/acp.rs`) — Cursor, Devin, Fx, Grok, Kimi

Goddard spawns the ACP agent process itself with a full env vector
(`acp.rs:219-272`), but **ACP the protocol has no subagent concept** —
`session/new` + `session/update` only; no delegation request, no subagent
`tool_call` kind. Subagent support is entirely up to the wrapped CLI honoring
its own agent files while running under `--acp`, which is unverified per
provider. Underlying mechanisms exist (Cursor `.cursor/agents/*.md`, Grok
`.grok/agents/*.md` + `[subagents.models]`, Kimi agent YAML) but injecting
them means writing into the user's repo or global config dir, and **Kimi
cannot select a subagent model at all** (inherits session model).

- v1 shape: **observation-only**. Surface any subagent-ish `tool_call`s as
  activities (the only channel that exists, `acp.rs:2146`); steer nudge works
  everywhere except Fx (`acp.rs:212`). Revisit per provider once we can test
  whether e.g. `cursor-agent acp` honors `.cursor/agents/`.

### Pi / Oh My Pi (`driver/pi.rs`)

No built-in subagent tool — upstream ships subagents as an *extension* that
spawns `pi -p` subprocesses, and agent defs (`.pi/agents/*.md`) support
`model:`/`thinking:`. Goddard already passes `--extension <file>` and
`--skill <file>` for Computer Use (`pi.rs:183-200`) — so a goddard-owned
extension implementing a `task` tool with per-tier model selection is fully
in-pattern, file lives in goddard-owned storage, zero repo pollution. Steer
supported (`pi.rs:757`).

### Amp (`driver/amp.rs`)

Amp has a `Task` tool and plugin-defined agents (`amp.createAgent({model})`),
plus `tool.call`/`tool.result` plugin hooks — but config lives in
`.amp/settings.json` or `~/.config/amp`, i.e. a write into user-owned space.
CLI args carry model/effort only (`amp.rs:60-90`). Defer; steer-nudge only.

### DeepSeek Harness (`driver/deepseek.rs`)

Already emits `jobs` → `BackgroundWorkKind::Subagent` (`deepseek.rs:1084-1120`)
and takes `agentPreset` at session create (`deepseek.rs:370-377`). Whether the
harness lets a session dispatch subagents on other models is a provider
question; keep as "already observable, dispatch TBD".

## Caps and enforcement — honest tiers

| Provider | Live `[cap: N/MAX]` annotation | Hard cap | Degrade |
|---|---|---|---|
| Claude | `PostToolUse` → `additionalContext` via `--settings` | `PreToolUse` deny | steer (yes, `claude.rs:541`) |
| Codex | hooks.json possible but global-scope + partial coverage | same | steer (`codex.rs:1000`) |
| OpenCode v1 | `tool.execute.after` plugin | `tool.execute.before` arg/deny | steer (`opencode.rs:714`) |
| OpenCode 2 | none reachable on adopted service | none | steer (`opencode2.rs:644`) |
| ACP / Amp / Pi / DeepSeek | none v1 (Pi could do it inside our extension) | none v1 | steer (all but Fx) |

A steer nudge ("you have exhausted `goddard-fast`; continue inline or escalate")
is a uniform degrade because every transport except Fx advertises
`supports_steer`.

## Feature shape — the minimal slice

**waku-core owns:**

1. `SubagentTierSpec` (tiers + enabled flag + routing mode) in waku-protocol,
   TS-exported; persisted in `AppSettings` beside `provider_binary_overrides`
   (`persistence.rs:294`). Per-provider tier→model resolution happens once at
   `ensure_driver`/start, not per frame.
2. One new `DriverStartOptions` field — `subagents: Option<SubagentSpec>` —
   not a `SessionOptions` field: injected agents/hints are launch-time
   artifacts, and none of the harnesses can re-inject mid-session anyway.
3. A shared routing-hint text template + the `goddard-<tier>` naming convention.
4. Tier attribution = `role` prefix match at render; no new event types.

**Per-driver owns** a `prepare_subagents(spec, …)` step inside `start()`:

| Driver | Mechanism | Writes where |
|---|---|---|
| claude | `--agents` JSON + `--append-system-prompt` (+ `--settings` hooks for caps) | nowhere — flags only |
| codex | `developerInstructions` on `thread/start`; tier TOMLs only if needed | `~/.codex/agents/goddard-*.toml` (opt-in) |
| opencode2 | `put_instruction_entry` routing hint | session instruction entry |
| opencode | `OPENCODE_CONFIG_CONTENT` env on pool acquire | server env |
| pi | `--extension` goddard-owned task-tool extension | goddard data dir |
| acp/amp/deepseek | no-op v1 | — |

**Stays harness-specific:** agent definition format, per-call model params,
hook/plugin APIs, read-only enforcement. Do not abstract these behind a
common "subagent config file" — they don't share a shape.

**Explicitly out of scope for the slice:** grader/definition-of-done layer,
escalation ladders, routing modes (budget/quality/deep), cost ceilings,
cross-task `goddard-agent create` delegation, mid-session tier changes.

## Risks / open questions

- **Adopted OpenCode 2 cannot install agents.** If tier dispatch requires
  goddard-defined agents, opencode2 sessions can only route to agents the user
  already configured. Options: accept hint-only routing (model tiering lost),
  or ask the service to define agents — no such route exists today.
- **Config writes into user-owned space.** `~/.codex/agents/goddard-*.toml` are
  global and persist after the session — visible in the user's own Codex
  runs. Namespacing + a cleanup pass mitigate, but prefer flag/env injection
  wherever it exists. Never write `.claude/agents/` or `.codex/agents/` into
  the user's repo.
- **Sessions outliving injection.** Claude `--agents` is per-process and
  re-applied on resume — clean. OpenCode 2 instruction entries persist
  server-side past a daemon crash (the resume path already tolerates stale
  `goddard-agent` instructions, `opencode2.rs:508`); use the same reconcile
  pattern for a `goddard-subagents` key.
- **Attribution without cooperation.** If the model spawns a built-in agent
  (`Explore`, `general`) instead of `goddard-*`, the run still renders — it just
  isn't tier-attributed. That's acceptable; the hint is advisory, not
  enforceable, on every harness except where we gate the tool itself.
- **Hook latency.** Claude `PreToolUse`/`PostToolUse` spawn a process per
  tool call — off the UI thread, but on the agent's critical path. Keep the
  handler a fast static binary (the `goddard-agent` shim precedent), and make
  caps opt-in.
- **Codex override bugs.** Role-layer dropping `model`/`reasoning_effort` and
  context-fork spawns rejecting overrides are live upstream issues; the
  hint-only strategy must be validated against the pinned Codex version
  before shipping.
- **ACP verification gap.** Whether wrapped CLIs honor their agent-file
  mechanisms under `--acp` is untested; one cheap probe per provider decides
  whether ACP moves past observation-only.
