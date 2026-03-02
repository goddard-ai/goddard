# Vision: `pi-loop`

## Purpose

`pi-loop` turns `pi-coding-agent` from a one-shot execution model into a long-running autonomous coding daemon with explicit pacing controls.

In one sentence:

> `pi-loop` is a TypeScript-first orchestration layer that repeatedly runs `pi-coding-agent` cycles under configurable safety and operational limits.

## Why this exists

Autonomous coding workflows need:
- repeatable long-running execution,
- configurable delays and throughput control,
- typed configuration that can be validated early,
- lightweight operational deployment (e.g. via `systemd`).

`pi-loop` provides those pieces without requiring users to build daemon logic themselves.

## Product pillars

1. **Autonomy**: Keep cycling without manual intervention.
2. **Control**: Rate limits prevent runaway usage.
3. **Type safety**: Config authoring should be IDE-friendly and validated.
4. **Operability**: CLI entry points make setup and daemonization straightforward.
5. **Extensibility**: Prompt strategy is a pluggable interface.

## Scope map

This vision is implemented by the following specs:

- [Product & user outcomes](./product.md)
- [Loop runtime semantics](./runtime-loop.md)
- [Configuration contract](./configuration.md)
- [CLI behavior](./cli.md)
- [Rate limiting model](./rate-limiting.md)
- [Non-goals and boundaries](./non-goals.md)

## Source grounding

This spec set is derived from:
- `README.md`
- `QUICK_START.md`
- `src/` implementation
- legacy proposal in `spec.md`

Where implementation and proposal differ, this spec prioritizes current shipped behavior unless explicitly marked as a planned evolution.
