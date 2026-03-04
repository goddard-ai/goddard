---
id: spec-manifest
status: ACTIVE
---

# Spec Manifest — Goddard

This file is the sole routing hub for the Goddard specification graph. It contains no domain logic.

Policy: links are prohibited in this manifest. Use the plain file paths below to navigate.

---

## Trailhead

Start at `spec/vision.md` to understand the mission, product pillars, and system layers. Then open only the paths relevant to your task.

---

## Domain Map

| Domain | Entry Node | What you will find |
|--------|------------|--------------------|
| Mission & Pillars | `spec/vision.md` | Why Goddard exists; the two usage modes; the full spec graph |
| User Outcomes | `spec/product.md` | Personas, jobs-to-be-done, MVP success criteria |
| System Architecture | `spec/architecture.md` | Tech stack, repo layout, component responsibilities |
| Data Flows | `spec/data-flows.md` | E2E request and event-propagation sequences |
| CLI — Interactive | `spec/cli/interactive.md` | `goddard login`, `pr create`, `stream`, `actions trigger` |
| CLI — Loop | `spec/cli/loop.md` | `goddard loop init`, `run`, `generate-systemd` |
| Runtime Loop | `spec/runtime-loop.md` | Cycle lifecycle, persistent context, failure model |
| Configuration | `spec/configuration.md` | Typed config shape, discovery order, validation rules |
| Rate Limiting | `spec/rate-limiting.md` | Cycle delay, ops throttling, token enforcement |
| Non-Goals | `spec/non-goals.md` | Explicit boundaries and excluded responsibilities |
| Decisions | `spec/adr/` | Architecture Decision Records |
