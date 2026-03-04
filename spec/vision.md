---
id: goddard-vision
status: ACTIVE
links:
  - type: Leads-To
    target: spec/manifest.md
  - type: Leads-To
    target: spec/product.md
  - type: Leads-To
    target: spec/architecture.md
  - type: Leads-To
    target: spec/data-flows.md
  - type: Leads-To
    target: spec/cli/interactive.md
  - type: Leads-To
    target: spec/cli/loop.md
  - type: Leads-To
    target: spec/runtime-loop.md
  - type: Leads-To
    target: spec/daemon/index.md
  - type: Relates-To
    target: spec/non-goals.md
---

# Vision: Goddard

## Mission

Goddard bridges local terminal workflows with GitHub operations and extends that bridge to autonomous AI execution.

> Goddard provides a framework-agnostic TypeScript SDK, a Cloudflare-powered real-time backend, and an orchestration runtime so developers and agents can create PRs, observe repository events, and act without leaving the terminal.

## The Problem

Developer execution is fragmented across GitHub UI, CI dashboards, and chat tools. AI agents also lack a principled runtime for safe, long-running or feedback-triggered operation.

Goddard addresses:
1. **Interactive gap:** terminal-native GitHub operations with minimal context switching.
2. **Autonomous gap:** explicit runtimes for repeated cycles and one-shot PR-feedback reactions.

## Product Pillars

| # | Pillar | Meaning |
|---|--------|---------|
| 1 | SDK-first | Capabilities live in `@goddard-ai/sdk`; consumers stay thin. |
| 2 | Real-time | Repository events stream to terminals with low latency. |
| 3 | Delegated identity | `goddard[bot]` acts on behalf of authenticated developers. |
| 4 | Autonomous control | Built-in orchestration runs `pi-coding-agent` with safety limits. |
| 5 | Type safety | APIs and configuration are TypeScript-first with runtime validation. |
| 6 | Operability | CLI and `systemd` outputs support practical deployment. |
| 7 | Edge-native | Backend runs on Workers + Durable Objects for global fan-out. |

## Usage Modes

### 1) Interactive Developer CLI

A human developer uses terminal commands to authenticate, create PRs, trigger workflows, and launch focused `pi` sessions.

See [`cli/interactive.md`](./cli/interactive.md).

### 2) Autonomous Agent Runtimes

An operator runs unattended execution modes:
- Loop mode for recurring `pi-coding-agent` cycles.
- Daemon mode for webhook-derived PR-feedback one-shot sessions.
- Configurable limits for cadence, operations, and tokens.

See [`runtime-loop.md`](./runtime-loop.md), [`cli/loop.md`](./cli/loop.md), and [`daemon/index.md`](./daemon/index.md).

Both modes consume the same SDK.

## Navigation

For complete graph routing, use [`manifest.md`](./manifest.md). This file defines intent; detailed behavior lives in domain nodes.
