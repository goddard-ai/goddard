# `spec/` Index

This directory defines the product intent and architectural specification for **Goddard**.

## Reading Order

Start at the top and traverse as deep as your task requires.

| Document | Purpose |
|----------|---------|
| [`vision.md`](./vision.md) | Mission, product pillars, system layers, spec map |
| [`architecture.md`](./architecture.md) | Components, data flows, technology stack, deployment path |
| [`product.md`](./product.md) | User outcomes, success criteria, MVP scope |
| [`runtime-loop.md`](./runtime-loop.md) | Autonomous loop lifecycle and status contract |
| [`configuration.md`](./configuration.md) | Typed config shape, discovery order, validation rules |
| [`cli.md`](./cli.md) | CLI command behavior and exit semantics |
| [`rate-limiting.md`](./rate-limiting.md) | Cycle delay, ops throttling, token enforcement |
| [`non-goals.md`](./non-goals.md) | Explicit boundaries and exclusions |

## Source Documents (Legacy)

The following files are the historical sources that were synthesized into this spec set. They are preserved for reference but are **not** the canonical source of intent:

- `build.md` — implementation architecture proposal for the Goddard platform.
- `old-cmd/spec/` — modular spec set for the original autonomous agent loop layer (formerly `pi-loop`).
