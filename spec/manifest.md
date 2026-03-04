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
| Mission & Pillars | `spec/vision.md` | Why Goddard exists and the operating modes |
| User Outcomes | `spec/product.md` | Personas, jobs-to-be-done, MVP success criteria |
| System Architecture | `spec/architecture.md` | Stack choices and component boundaries |
| Data Flows | `spec/data-flows.md` | High-level request/event sequences |
| CLI — Interactive | `spec/cli/interactive.md` | `login`, `pr create`, `spec`, `propose`, `agents init` |
| Daemon — PR Feedback | `spec/daemon/index.md` | Feedback-triggered one-shot `pi` execution |
| CLI — Loop | `spec/cli/loop.md` | `loop init`, `run`, `generate-systemd` |
| Runtime Loop | `spec/runtime-loop.md` | Cycle lifecycle and termination model |
| Configuration | `spec/configuration.md` | Typed config contract and discovery order |
| Rate Limiting | `spec/rate-limiting.md` | Cadence, ops throughput, token limits |
| Non-Goals | `spec/non-goals.md` | Explicit boundaries |
| Decisions | `spec/adr/` | Architecture decision records |
