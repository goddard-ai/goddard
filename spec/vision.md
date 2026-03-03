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
    target: spec/daemon/index.md
---

# Vision: Goddard

## Mission

**Goddard** is a real-time developer tooling platform that bridges local terminal workflows with GitHub operations — and extends that bridge to fully autonomous, long-running agent loops.

In one sentence:

> Goddard provides a framework-agnostic TypeScript SDK, a Cloudflare-powered real-time backend, and an orchestration runtime so that developers — and autonomous AI agents — can create PRs, watch repository events, and act on them without leaving the terminal.

---

## The Problem

Modern developer workflows split across too many surfaces: GitHub UI for PR review, CI dashboards for build status, chat for async discussion. There is no programmable, terminal-native layer that surfaces those events in real time, nor an opinionated way to drive an AI coding agent against a repository continuously and safely.

Goddard solves both:

1. **The interactive gap** — developers need terminal-native GitHub operations (auth, PR creation, workflow triggers) without context switching.
2. **The autonomous gap** — AI coding agents need principled runtimes for both long-running cycles and feedback-triggered one-shot reactions.

---

## Product Pillars

| # | Pillar | What it means |
|---|--------|---------------|
| 1 | **SDK-first** | All capabilities live in `@goddard-ai/sdk`. CLI and agent loops are thin consumers. |
| 2 | **Real-time** | Repository events stream to connected terminals with sub-second latency via Server-Sent Events (SSE). |
| 3 | **Delegated identity** | The Goddard GitHub App (`goddard[bot]`) acts on behalf of authenticated developers. |
| 4 | **Autonomous control** | A built-in orchestration layer runs `pi-coding-agent` cycles under configurable safety limits. |
| 5 | **Type safety** | Configuration and APIs are TypeScript-first with Zod validation and IDE completion. |
| 6 | **Operability** | CLI entry points and `systemd` unit generation make deployment straightforward. |
| 7 | **Edge-native** | The backend runs on Cloudflare Workers + Durable Objects for globally low-latency distribution. |

---

## System Layers

```
┌─────────────────────────────────────────────────────────┐
│  Consumers                                              │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐  │
│  │  cmd/ (CLI)  │  │ agent loops  │  │ 3rd-party    │  │
│  │  interactive │  │ autonomous   │  │ integrations │  │
│  └──────┬───────┘  └──────┬───────┘  └──────┬───────┘  │
│         └─────────────────┴─────────────────┘           │
│                           │                             │
│               ┌───────────▼───────────┐                 │
│               │   @goddard-ai/sdk     │                 │
│               │  TokenStorage DI      │                 │
│               │  sdk.pr.create()      │                 │
│               │  sdk.stream.subscribe │                 │
│               └───────────┬───────────┘                 │
└───────────────────────────┼─────────────────────────────┘
                            │ HTTPS / SSE
┌───────────────────────────▼─────────────────────────────┐
│  Backend (Cloudflare Workers + Durable Objects)          │
│  Auth · PR creation · Webhook ingest · Stream broadcast  │
│                       │                                  │
│              Turso (SQLite at Edge)                      │
└───────────────────────┬─────────────────────────────────┘
                        │ Webhooks / GitHub API
                ┌───────▼────────┐
                │  GitHub        │
                │  App + API     │
                └────────────────┘
```

---

## Two Usage Modes

### Mode 1: Interactive Developer CLI

A human developer authenticates once, then uses the terminal to:
- Create PRs attributed to `goddard[bot]` on their behalf.
- Trigger GitHub Actions workflows.
- Launch focused `pi` sessions for specification and proposal tasks.

→ See [`cli/interactive.md`](./cli/interactive.md) for the full command specification.

### Mode 2: Autonomous Agent Runtimes

An operator can run autonomous processes that:
- Repeatedly drive a `pi-coding-agent` session against a codebase (loop mode).
- Listen to webhook-derived PR feedback events and trigger local one-shot `pi` sessions (daemon mode).
- Enforce safety limits (token budgets, operation caps, cycle delays) where applicable.

→ See [`runtime-loop.md`](./runtime-loop.md), [`cli/loop.md`](./cli/loop.md), and [`daemon/index.md`](./daemon/index.md) for details.

Both modes consume the same `@goddard-ai/sdk`.

---

## Spec Map

| Document | Scope |
|----------|-------|
| [`manifest.md`](./manifest.md) | Routing hub — start here to navigate the graph |
| [`product.md`](./product.md) | User outcomes, success criteria, MVP scope |
| [`architecture.md`](./architecture.md) | System components, technology choices, deployment model |
| [`data-flows.md`](./data-flows.md) | E2E request and event-propagation sequences |
| [`cli/interactive.md`](./cli/interactive.md) | Interactive CLI command behavior |
| [`daemon/index.md`](./daemon/index.md) | Daemon behavior for feedback-triggered one-shot sessions |
| [`cli/loop.md`](./cli/loop.md) | Autonomous loop CLI command behavior |
| [`runtime-loop.md`](./runtime-loop.md) | Loop lifecycle, context model, failure semantics |
| [`configuration.md`](./configuration.md) | Typed config contract, validation, discovery |
| [`rate-limiting.md`](./rate-limiting.md) | Cycle delay, ops throttling, token enforcement |
| [`non-goals.md`](./non-goals.md) | Explicit boundaries and exclusions |
| [`adr/`](./adr/) | Architecture Decision Records |
