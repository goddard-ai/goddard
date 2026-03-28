# Goddard Spec Root

## Mission

Goddard unifies local repository workflows with GitHub operations and extends that bridge to autonomous AI execution.

> Goddard provides a framework-agnostic TypeScript daemon control-plane SDK, a Cloudflare-powered real-time backend, and a Tauri desktop workspace so developers and agents can control local automation and act from shared local context.

## The Problem

Developer execution is fragmented across GitHub UI, IDEs, chat tools, and ad hoc local scripts. AI agents also lack a principled runtime for safe, long-running or feedback-triggered operation.

Goddard addresses:
1. **Workspace gap:** a unified local surface for GitHub operations and AI steering with minimal context switching.
2. **Autonomous gap:** explicit runtimes for repeated cycles, daemon-managed workforce delegation, and one-shot pull-request feedback reactions.

## Product Pillars

| # | Pillar | Meaning |
|---|--------|---------|
| 1 | SDK-first | Daemon control capabilities live in `@goddard-ai/sdk`; consumers stay thin. |
| 2 | Real-time | Managed pull request events stream to desktop workspaces and background runtime hosts with low latency. |
| 3 | Delegated identity | `goddard[bot]` acts on behalf of authenticated developers. |
| 4 | Autonomous control | Built-in orchestration runs `pi-coding-agent` with safety limits. |
| 5 | Type safety | APIs are TypeScript-first, while configuration remains machine-readable and validated at runtime. |
| 6 | Operability | Daemon-managed local execution is first-class, while desktop, SDK, and narrow operational CLI clients remain valid control surfaces. |
| 7 | Edge-native | Backend runs on Workers for global fan-out. |

## Usage Modes

### 1) SDK Integrations
A developer or product integration uses `@goddard-ai/sdk` directly to authenticate through the daemon, control daemon-managed sessions and automation, and embed local control surfaces into custom hosts.

### 2) Desktop Workspace
A human developer uses a unified, IDE-like desktop surface to monitor sessions, review pull requests, browse specs, and manage roadmap context without hopping across multiple tools.

### 3) Background Automation
A local runtime hosted by the desktop app or another supervised local process handles unattended execution:
- Loop mode for recurring `pi-coding-agent` cycles.
- Pull-request feedback one-shot handling triggered by managed pull request events.
- Repository-scoped workforce orchestration for delegated multi-agent work.
- Configurable limits for cadence, operations, and tokens.

### 4) Operational CLI
A thin operator-focused CLI may initialize repository-local automation intent and inspect or control supported daemon-backed runtimes without becoming a parallel primary UX.

All modes share the same backend authority model, with the SDK serving as the daemon control plane where programmatic control is needed.

## Encapsulated Sub-Specs

* `spec/configuration.md`: Configuration hierarchy, precedence, and named configurable entities.
* `spec/core.md`: Core system runtime shared by SDK, app, and background automation.
* `spec/daemon.md`: Daemon-managed background automation runtimes and their shared constraints.
* `spec/app.md`: Desktop application UX and features.
* `spec/cli.md`: Narrow operational CLI role and removed broad terminal workflows.
* `spec/adr/`: Architecture decision records.
