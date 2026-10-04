# Boss capability gating

## Decision

Adopt a boss-only capability profile that removes arbitrary execution, project
file tools, direct internet tools, and native delegation. Keep typed boss
operations, transcript lookup, and a bounded daemon-owned verification surface.
Apply the same profile to planning sessions. Employees retain their normal
execution capabilities and existing persona grants.

This is a recommendation, not implemented behavior. Do not remove execution
first: boss operations currently travel through the `goddard-agent` CLI, so doing
so would also remove delegation and memory maintenance. Introduce a direct tool
transport before enforcing the new profile. Read-only execution is a possible
temporary compatibility mode, but does not make delegation structurally required.

## Evidence scope

Investigation date: 2026-10-04. The assigned checkout is `main` at
`8c51c3ab175ee4c62c2a8084ef47fd8dd034bc3b`; it does not contain the boss service.
The current development source inspected read-only is
`/Users/alec/dev/worktrees/goddard/dev` at
`8673998cce1b3416bbb723817b60b6023a52c601` (**D** below). The live daemon's
`boss view` returned file paths under that worktree's `temp/boss/files`.
This establishes the relevant data root, not the exact binary build revision.

Planning sessions are separate proposed work, absent from D. Their source was
inspected at
`/Users/alec/dev/goddard-ai/goddard/worktrees/goddard/planning-sessions/goddard`,
commit `1591aa1e7c7cd5c988853202f106787e3edb45d8` (**P** below).
File:line references are relative to those explicit revisions. No provider
launches, visual inspection, or live boss tool enumeration were performed.
Statements about available native tools mean the inherited harness surface;
the exact names and enabled tools depend on provider version and user config.

## Current session construction

1. `BossOperation::Open` is human-only and stores the supplied `RuntimeMode`
   on an ordinary `AgentSession` marked `boss_managed`. It creates a Boss project
   at the daemon-owned workspace, rather than introducing a restricted runtime
   type (D `crates/waku-core/src/daemon.rs:6063–6115`).
2. Cold start constructs ordinary `DriverStartOptions`, taking
   `mode: session.runtime_mode` (D `daemon.rs:5345–5370`). `spawn_runtime` records
   project context and changes the boss cwd to `BossService::workspace`; that
   resolves to `<boss root>/workspace`, distinct from `<boss root>/files`.
   Changing cwd is not filesystem isolation (D `daemon.rs:4974–4991`,
   `boss.rs:361–374`).
3. Managed sessions require a scoped agent launch credential. Task tools are
   advertised even when `agent_tools_enabled` is off; settings writes are off
   for managed roles. Scope includes the boss flag, not a general tool allowlist
   (D `daemon.rs:5040–5044,5283–5293`, `agent.rs:679–702`).
4. The role/persona is assembled as a `<boss-persona>` prompt prefix once per
   injection cycle. The text explicitly says broader filesystem editing and
   internet access are “discouraged, not forbidden”; it also requires verification
   from worktrees and commits (D `boss.rs:440–489`). The ordinary agent surface
   is delivered through provider-specific instructions: Codex
   `developerInstructions`, Claude `--append-system-prompt`, or OpenCode's
   session instruction entry and credential-carrying launcher
   (D `driver/codex.rs:539–586`, `driver/claude.rs:247–271`,
   `driver/opencode.rs:612–634`). These instructions add behavior, not tool
   restrictions.
5. Goddard does not inject its configured native subagent roster into managed
   roles. This does not prove the provider's own built-in delegation tool is
   absent. Computer Use is disabled for the boss because the managed-role clamp
   requires an employee grant. Connected integration MCP servers are restricted
   to employee integration grants; the boss has no employee record, hence no
   grants through this path (D `daemon.rs:4987–4990,5064–5082,5105–5124`).

There is no boss capability profile or native tool allowlist field in
`DriverStartOptions` (D `driver/mod.rs:308–365`). Therefore a boss can inherit
ordinary command, read, edit, and web capabilities wherever its harness and
access mode provide them. This is a source-level conclusion, not an exhaustive
runtime tool inventory across every provider.

## Surface inventory and legitimate uses

| Surface today | Intended use | Current boundary | Proposed treatment |
| --- | --- | --- | --- |
| `boss view`, `context` | Employee roster, work digest, project/task/automation context | Daemon role checks | Keep |
| `summon`, `control`, persona management, identity/icon operations | Delegate and steer; maintain reusable roles and grants | Role authorization, active employees, grant clamping | Keep |
| `transcript`; CLI `search` and `read` | Inspect outcomes and past human work | Transcript authorization; boss search spans registered projects | Keep through typed tools, not shell |
| `listFiles`, `readFile`, `writeFile`, `createFolder`; `memory` | Boss-owned documents, persistent facts, memory indexing | Relative files-root paths; employee grants; symlink rejection | Keep through boss operations |
| `eval` | Batch and chain boss operations; retain script variables | Rhai budgets and ordinary operation authorization | Keep; do not add process, network, or arbitrary file bindings |
| Bundle publishing and lifecycle; `speak` | Expose employee artifacts and notify the human | Owner-only operations; bundle path names a daemon-host artifact | Keep; artifact publication does not require shell |
| General command tools (`exec`, Bash, harness equivalents) | Today: run CLI boss ops, inspect Git/worktree, search project tree | Ordinary session access mode | Replace CLI dependence; remove arbitrary commands |
| Native file read/search/edit/patch tools | Today: project investigation, direct verification, broader editing | Provider filesystem/access policy, not boss files ACL | Remove; delegate investigation and implementation |
| Native web/search/network tools | Today: internet research if harness exposes it | Harness/config dependent; discouraged in persona | Remove; delegate research |
| Native subagents | Harness-level delegation outside Boss employee lifecycle | Goddard roster withheld; persona says use Boss summon | Explicitly disable where supported |
| Connected integration MCP / Computer Use | Employee external services / desktop work | Boss receives no integration grants through managed path; Computer Use clamped off | Preserve absence; prevent other config routes from reintroducing them |
| Generic task/settings/resource CLI operations | Normal task creation, settings, resource reservations | Boss `create`, settings changes, identity rename, resource acquire already rejected | Preserve server rejection; expose only useful bounded reads |

Inventory evidence: D `crates/waku-protocol/src/boss.rs:183–325` owns the operation
set; `boss_eval.rs:42–72` lists eval bindings. D `boss.rs:1198–1268` owns files
authorization/path checks; `daemon.rs:6361–6394` owns transcript, context, eval;
`daemon.rs:6944–6957` gives boss-wide search. Existing rejection paths are
`daemon.rs:3375–3388,4621–4624,4873–4879,5415–5418`. General tool *names* in the
table are examples rather than a single provider-neutral list.

### What remains necessary outside existing boss operations?

The main gap is **independent verification**. The current instruction asks the
boss to verify actual worktree contents and commits, not merely accept an
employee summary (D `boss.rs:468`). A transcript is evidence of what an employee
said or ran, not proof of the current repository state. Removing all project
reads without replacing this mechanism would weaken that requirement.

Provide an operation such as `inspectEmployeeWork` keyed by employee session id,
not arbitrary command or filesystem path. Resolve the owned workspace server-side
and return bounded status, HEAD, requested commit metadata/diff, and changed file
excerpts. Report missing, dirty, or changed workspaces explicitly. Include the
observed revision/time and mark the result as a snapshot. A subsequent integration
can change it; this is verification evidence, not an atomic deployment guarantee.
An independent Verifier employee can perform tests and substantive review.

Reading the live project tree also helps the boss scope a job quickly. That is
a latency/cost benefit, not a requirement for coordination: a Researcher employee
can locate code and report evidence. Avoid adding arbitrary project read/search
back into the boss profile under a different tool name. If narrow file excerpts
are needed to assess an employee's delivery, attach them to the verification
operation and bound them to that employee's workspace.

Memory maintenance needs no general editor: boss files and deterministic memory
operations already exist. Files use relative paths and reject symlink components
(D `boss.rs:1253–1268`). `publishBundle` accepts an absolute artifact path and
checks its metadata through the daemon; the boss does not have to run a process
to publish it (D `boss.rs:1048–1067`). Internet access is already assigned to
employees by the default persona (D `boss.rs:1538`). Transcript discovery is a
legitimate coordination need and should survive even though `search/read` are
currently CLI commands outside `BossOperation`.

## Enforcement mechanisms and limits

**Daemon authorization is real, but covers daemon operations.** The caller's
scoped identity reaches `handle_boss_operation`; authorization there prevents
employees from expanding grants or modifying boss state. It does not intercept
arbitrary filesystem access by the provider process (D `daemon.rs:1914–1916`,
`boss.rs:1198–1268`). Boss `eval` retains operation authorization, has a 30-second
budget, and disables module imports; its exposed bindings are boss operations,
not host shell access (D `boss_eval.rs:1–11,23–38,184–214`).

**Access mode is not role policy.** Codex maps `Ask` to
`approvalPolicy=untrusted` / `sandbox=read-only`, `AutoAcceptEdits` to
`on-request` / `workspace-write`, `Auto` to `on-request` / `workspace-write` with
`auto_review`, and `FullAccess` to `never` / `danger-full-access`. This mapping
also feeds per-turn `sandboxPolicy` (D `driver/codex.rs:1316–1345`). Merely selecting
Ask still allows escalation and does not remove read or command tools.

Claude launches `--permission-prompt-tool stdio` and `--permission-mode`, with
`--dangerously-skip-permissions` in FullAccess. No boss-specific tool allowlist is
assembled there (D `driver/claude.rs:99–106,150–172,227–271`). Pi/Oh My Pi currently
accept only FullAccess (`--approve` / `--yolo`); changing their access mode is not
a read-only fallback (D `driver/pi.rs:63–69,216–220`).

**ACP is not a universal execution firewall.** Goddard advertises
`ClientCapabilities::terminal(false)` and answers provider permission requests.
That capability says the client does not supply an ACP terminal; it does not
disable the agent's own Bash/filesystem tools. FullAccess auto-approves received
permission requests; other modes prompt or review them (D
`driver/acp.rs:639–663,769–784,843–857`). Events reporting tool execution are
observations, not necessarily interception before execution. An adapter must
demonstrate actual removal/denial before claiming the profile is enforced.

Auto-review is not a substitute: its question is malicious activity, and
explicitly permits non-malicious task-conflicting actions (D
`permission_review.rs:59–63`). Capability denials must precede that review and
must not be overridden by user approval or a FullAccess setting.

Also audit inherited provider MCP/config/extensions: restricting Goddard's
integration list does not establish the absence of externally configured tools.
Do not mutate global provider config or OpenCode's shared permission store to
implement a single-session policy. Its agent credential already uses a
session-specific launcher because the native service is shared (D
`driver/opencode.rs:612–634`). Provider flags or APIs proposed below require
validation against the installed provider; no unverified new flag is assumed.

## Three designs

| Design | Benefit | Limitation / cost |
| --- | --- | --- |
| Instruction-only status quo | No adapter work; boss can inspect and recover directly; all supported harnesses retain compatibility | Delegation depends on compliance; commands can block; broader edits and research remain possible |
| Hard removal except coordination/boss operations | Execution and research require an employee; clearer ownership; fewer bypasses of files ACLs and plan freezes | Needs direct boss transport, transcript lookup, verification snapshots, and per-provider enforcement; extra employee latency and cost |
| Read-only exec + no edit tools | Preserves direct code/Git inspection; can reduce accidental repository modification | Shell still researches, runs costly computations, waits, and may contact external services; genuine read-only enforcement needs sandboxing and denied escalation; not portable through current access modes |

Read-only shell policy must not mean a command-name denylist. Shell redirects,
interpreters, subprocesses, hooks, HTTP requests, and local IPC make that an
incomplete boundary. Even filesystem-enforced read-only execution can perform
external writes and block unless network/IPC and time/output are bounded too.
Boss operations themselves legitimately write daemon-owned memory; they should
remain authorized out-of-process rather than require an exception for arbitrary
shell writes.

Recommend the hard design as the end state. The middle path is useful only as an
explicit interim compatibility mode, labelled as such, with no claims of forced
delegation. If a provider cannot enforce the hard profile, fail the boss launch
with an actionable unsupported-provider error. Offer a normal employee/task
runtime for that provider; do not silently restore unrestricted boss access.
Do not introduce a routine “allow once” execution escape hatch for bosses.

## Planning and remote sessions

At P, `is_boss_principal` includes planning sessions; `is_managed` includes boss,
employee, and planning. Planning gets the boss persona and a specific
`memory/plans/...` document to write via `writeFile`, then `finalizePlan` for user
approval (P `boss.rs:191–218,593–606`, `daemon.rs:4897–4987`). Therefore use
`is_boss_principal` for capability selection, not just `is_boss`, and not
`is_managed` (which would remove employees' tools).

P rejects writes to frozen plan documents through boss ops
(`boss.rs:1171–1172`). **Inference:** a provider with unrestricted file editing
could bypass that API check by writing the physical file directly. Removing
general edits closes that route for the planning principal, but is not protection
against host processes or unrelated unrestricted employees. Preserve daemon
freeze enforcement and test normalized path spellings, lifecycle transitions,
and eval calls. Archived planning records must not regain authority on resume;
P already rejects archived planning delegation (`boss.rs:380–387`).

Remote Boss is another daemon's boss, not a boss process on the client machine.
The app selects a supervisor by `DaemonKey` for boss requests (D
`src/app/boss.rs:386–403`); the architecture and connection boundaries are in D
`.agents/docs/remote-boss.md`. Enforce capabilities at the owning daemon's launch
path so desktop, remote clients, cold starts, and headless prompts get the same
profile. Do not rely on hiding tools or controls in the local UI. Keep tokens,
files, employee verification paths, and scoped tool endpoints on that daemon.

Hosted provider Cloud environment is different: managed-role launches currently
fail with “Boss roles require a provider runtime that can reach this daemon”
(D `daemon.rs:5028–5038`). This proposal does not add hosted boss support or
cross-daemon boss messaging. Sandbox guests and remote hosts must each be tested
for daemon reachability and capability enforcement; never fall back to a local
unrestricted runtime when setup fails.

## Phased implementation sketch

1. **Define the invariant and adapter contract.** Add a daemon-owned
   `SessionCapabilities`/`ToolProfile` to `DriverStartOptions`, separate from
   `RuntimeMode`. Derive `BossCoordinator` for boss principals at `spawn_runtime`;
   keep employee execution unchanged. Include capability support discovery and
   a versioned effective profile in diagnostics. Inventory each provider's native
   tools, inherited config, and extension paths before enabling it.
2. **Remove the shell dependency.** Expose typed boss operations through a
   session-scoped tool transport (for example MCP), plus transcript search/read
   and schema/help. Route all calls through current caller authorization. Keep
   `eval` bounded and boss-operation-only. Bind credentials server-side to the
   session; do not accept arbitrary caller/session ids from tool payloads.
3. **Replace direct verification.** Add bounded `inspectEmployeeWork` snapshots
   with server-resolved workspace ownership, revision, dirty status, commit/diff,
   and capped excerpts. Delegate tests, builds, integration, and extended review.
   Define unavailable/deleted workspace errors and require fresh evidence when
   the claimed revision changes. Preserve memory and artifact operations.
4. **Enforce in supported adapters.** Remove shell/read/edit/web/native delegation
   tools and reject forbidden invocations at execution boundaries. Configure
   session-owned tool sets, never global settings. Audit native REPL, MCP,
   background command and continuation paths. In adapters without full tool
   interception/removal, use process isolation only if it proves the same
   invariant; otherwise refuse the profile. Initially keep instruction-only or
   read-only compatibility an explicit rollout choice, not silent fallback.
5. **Make lifecycle behavior consistent.** Apply policy to fresh starts, resumes,
   forks/provider switches, retries, options changes, rotations, and remote
   daemon sessions. Recreate provider threads when immutable launch settings
   cannot be tightened on resume. Never let a saved FullAccess mode or planning
   archival change restore generic tools. Update persona/tool guidance to match
   the enforced surface.
6. **Verify before defaulting on.** Use provider-level negative probes, not just
   prompt-compliance tests: arbitrary shell and out-of-root reads/edits fail;
   native web/MCP/REPL/delegation cannot restore execution; resumed sessions keep
   restrictions. Positive probes cover summon/control, memory, transcript lookup,
   verification, publishing, speech, and P's draft/finalize/frozen-plan lifecycle.
   Exercise the same cases through local and remote daemon paths. Record denied
   capability attempts and unsupported-provider failures without secret payloads.

The deciding tradeoff is added delegation latency versus a dependable coordinator
role. With typed operations and independent verification in place, general tools
are not necessary for that role. Without those replacements, hard removal would
break coordination and reduce evidence quality rather than improve delegation.
