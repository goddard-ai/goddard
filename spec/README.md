# Goddard Spec Root

## Mission

Goddard unifies local repository workflows with GitHub operations and extends that bridge to autonomous AI execution.

> Goddard provides a framework-agnostic TypeScript SDK, a Cloudflare-powered real-time backend, and a Tauri desktop workspace so developers and agents can create PRs, observe managed pull request events, and act from shared local context.

## The Problem

Developer execution is fragmented across GitHub UI, IDEs, chat tools, and ad hoc local scripts. AI agents also lack a principled runtime for safe, long-running or feedback-triggered operation.

Goddard addresses:
1. **Workspace gap:** a unified local surface for GitHub operations and AI steering with minimal context switching.
2. **Autonomous gap:** explicit runtimes for repeated cycles and one-shot PR-feedback reactions.

## Product Pillars

| # | Pillar | Meaning |
|---|--------|---------|
| 1 | SDK-first | Capabilities live in `@goddard-ai/sdk`; consumers stay thin. |
| 2 | Real-time | Managed pull request events stream to desktop workspaces and SDK consumers with low latency. |
| 3 | Delegated identity | `goddard[bot]` acts on behalf of authenticated developers. |
| 4 | Autonomous control | Built-in orchestration runs `pi-coding-agent` with safety limits. |
| 5 | Type safety | APIs are TypeScript-first, while configuration remains machine-readable and validated at runtime. |
| 6 | Operability | Desktop-managed local execution is first-class, while SDK-based hosts and external supervisors remain valid deployment options. |
| 7 | Edge-native | Backend runs on Workers for global fan-out. |

## Usage Modes

### 1) SDK Integrations
A developer or product integration uses `@goddard-ai/sdk` directly to authenticate, create pull requests, subscribe to managed pull request events, and embed Goddard capabilities into custom hosts.

### 2) Desktop Workspace
A human developer uses a unified, IDE-like desktop surface to monitor sessions, review pull requests, browse specs, and manage roadmap context without hopping across multiple tools.

### 3) Background Automation
A local runtime hosted by the desktop app or another SDK consumer handles unattended execution:
- Loop mode for recurring `pi-coding-agent` cycles.
- PR-feedback one-shot handling triggered by managed pull request events.
- Configurable limits for cadence, operations, and tokens.

All modes consume the same SDK and backend authority model.

## Encapsulated Sub-Specs

* `spec/configuration.md`: Configuration hierarchy, precedence, and named configurable entities.
* `spec/core.md`: Core system runtime shared by SDK, app, and background automation.
* `spec/daemon.md`: Background automation and daemon functionality.
* `spec/app.md`: Desktop application UX and features.
* `spec/cli.md`: Decommissioned CLI surface and removal notes.
* `spec/adr/`: Architecture decision records.
