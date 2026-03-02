# Product Specification

## Primary user

A developer/operator who wants `pi-coding-agent` to run continuously against a codebase with bounded operational behavior.

## Core jobs-to-be-done

1. Initialize a usable config quickly.
2. Run autonomous cycles indefinitely.
3. Adjust prompt strategy without modifying loop internals.
4. Control cadence and operation rate to avoid excessive spend/load.
5. Deploy as a service in environments that use `systemd`.

## Key user outcomes

- User can run `pi-loop init` and immediately get a valid typed config.
- User can run `pi-loop run` and have local config automatically discovered.
- User can choose global config fallback when local config is absent.
- User receives deterministic command-line errors when config is missing/invalid.

## Success criteria (MVP)

- CLI commands: `init`, `run`, `generate-systemd`.
- Public API exports `createLoop` and `createLoopConfig`.
- Strategy contract supports custom classes implementing `nextPrompt(ctx)`.
- Runtime performs repeated cycles and carries forward prior summary context.

## Planned evolution (not guaranteed by current runtime)

The original proposal in `spec.md` includes richer resource and token controls. Those are considered forward-looking and should be added incrementally after explicit implementation:

- stricter token-budget enforcement,
- richer metrics/observability,
- stronger model/config bridging into agent runtime,
- explicit cycle termination protocol (e.g. handling `DONE`).
