# Vision: Goddard & pi-loop

## Purpose

Goddard is a modern, real-time developer tooling suite designed to bridge local terminal workflows with GitHub operations, while incorporating `pi-loop` to turn one-shot autonomous execution into a long-running, autonomous coding daemon with explicit pacing controls.

In one sentence:

> Goddard is a TypeScript-first orchestration layer and developer tooling suite that seamlessly integrates real-time GitHub operations with an endlessly running `pi-coding-agent` loop under configurable safety and operational limits.

## Why this exists

Modern developer workflows and autonomous coding agents require:
- **Seamless Integration**: A framework-agnostic TypeScript SDK and a thin CLI wrapper to bridge terminal workflows with GitHub operations (e.g., creating PRs, triggering Actions, real-time streaming updates).
- **Repeatable Autonomy**: Long-running execution of coding agents without manual intervention.
- **Configurable Control**: Delays, throughput control, and rate limits to prevent runaway usage.
- **Type Safety**: IDE-friendly and validated configuration authoring.
- **Operability**: Straightforward CLI entry points for setup and daemonization (e.g., via `systemd`).

Goddard and `pi-loop` provide these pieces, preventing users from needing to build daemon logic and GitHub integrations themselves.

## Product pillars

1. **Autonomy**: Keep coding agent cycles running without manual intervention.
2. **Real-time Connectivity**: Stream repository events directly to the developer's terminal.
3. **Control**: Rate limits prevent runaway usage, and delegated PR creation ensures actions are attributable.
4. **Type safety**: Config authoring should be IDE-friendly and validated.
5. **Operability**: CLI entry points make setup and daemonization straightforward.
6. **Extensibility**: Prompt strategy is a pluggable interface, and SDK-first design allows third-party integrations.

## Scope map

This vision is implemented by the following specs:

- [Product & user outcomes](./product.md)
- [Architecture & Implementation](./architecture.md)
- [Loop runtime semantics](./runtime-loop.md)
- [Configuration contract](./configuration.md)
- [CLI behavior](./cli.md)
- [Rate limiting model](./rate-limiting.md)
- [Deployment & Build Status](./deployment.md)
- [Non-goals and boundaries](./non-goals.md)

## Source grounding

This spec set is synthesized from:
- `build.md` (Project Goddard Architecture & Implementation Proposal)
- `old-cmd/spec/vision.md` (Original `pi-loop` Vision)
- `README.md`
- `QUICK_START.md`
- `src/` implementation

Where implementation and proposal differ, this spec prioritizes current shipped behavior unless explicitly marked as a planned evolution.
