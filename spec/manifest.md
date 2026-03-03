---
id: spec-manifest
status: ACTIVE
---

# Spec Manifest — Goddard

This file is the **sole routing hub** for the Goddard specification graph. It contains no domain logic. Follow typed links to the node that covers your task domain.

---

## Trailhead

Start at [`vision.md`](./vision.md) to understand the mission, product pillars, and system layers. Then follow only the links relevant to your task.

---

## Domain Map

| Domain | Entry Node | What you will find |
|--------|------------|--------------------|
| **Mission & Pillars** | [`vision.md`](./vision.md) | Why Goddard exists; the two usage modes; the full spec graph |
| **User Outcomes** | [`product.md`](./product.md) | Personas, jobs-to-be-done, MVP success criteria |
| **System Architecture** | [`architecture.md`](./architecture.md) | Tech stack, repo layout, component responsibilities |
| **Data Flows** | [`data-flows.md`](./data-flows.md) | E2E request and event-propagation sequences |
| **CLI — Interactive** | [`cli/interactive.md`](./cli/interactive.md) | `goddard login`, `pr create`, `stream`, `actions trigger` |
| **CLI — Loop** | [`cli/loop.md`](./cli/loop.md) | `goddard loop init`, `run`, `generate-systemd` |
| **Runtime Loop** | [`runtime-loop.md`](./runtime-loop.md) | Cycle lifecycle, persistent context, failure model |
| **Configuration** | [`configuration.md`](./configuration.md) | Typed config shape, discovery order, validation rules |
| **Rate Limiting** | [`rate-limiting.md`](./rate-limiting.md) | Cycle delay, ops throttling, token enforcement |
| **Non-Goals** | [`non-goals.md`](./non-goals.md) | Explicit boundaries and excluded responsibilities |
| **Decisions** | [`adr/`](./adr/) | Architecture Decision Records |
