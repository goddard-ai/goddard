# Product Specification

## Primary user

A developer/operator who wants a seamless terminal experience for GitHub operations, and to optionally run `pi-coding-agent` continuously against a codebase with bounded operational behavior.

## Core jobs-to-be-done

1. Delegate PR creation via CLI while preserving explicit human attribution.
2. Stream repository events (comments, reviews) directly into the local terminal in real-time.
3. Initialize a usable configuration quickly for autonomous coding.
4. Run autonomous agent cycles indefinitely.
5. Adjust prompt strategies without modifying core internals.
6. Control loop cadence and operation rate to avoid excessive spend/load.
7. Deploy the agent as a service in environments that use `systemd`.

## Key capabilities

- **Delegated PR Creation:** The CLI creates PRs via the GitHub App identity (`goddard[bot]`), while explicitly mentioning the authenticated human developer responsible for the action.
- **Real-Time Terminal Streaming:** The CLI subscribes to repository events (comments, reviews) via WebSockets, streaming them directly into the developer's terminal in real-time.
- **Automated Reactions:** The GitHub App automatically reacts (e.g., 👀 emoji) to comments/reviews on PRs it manages.
- **SDK-First Design:** All capabilities are exposed via a TypeScript SDK, allowing third parties to build custom automations, bots, or GUIs without relying on CLI subprocesses.

## Key user outcomes

- User can trigger standard GitHub operations (`login`, `pr create`, `actions trigger`, `stream`) from the terminal.
- User can run `pi-loop init` and immediately get a valid typed config.
- User can run `pi-loop run` and have local config automatically discovered.
- User can choose global config fallback when local config is absent.
- User receives deterministic command-line errors when config is missing/invalid.

## Success criteria (MVP)

- CLI commands for GitHub: `login`, `pr`, `actions`, `stream`.
- CLI commands for automation: `init`, `run`, `generate-systemd`.
- Public API exports `createLoop` and `createLoopConfig` via the SDK.
- Strategy contract supports custom classes implementing `nextPrompt(ctx)`.
- Runtime performs repeated cycles and carries forward prior summary context.

## Planned evolution (not guaranteed by current runtime)

The original proposal includes richer resource and token controls. Those are considered forward-looking and should be added incrementally after explicit implementation:

- stricter token-budget enforcement,
- richer metrics/observability,
- stronger model/config bridging into agent runtime,
- explicit cycle termination protocol (e.g. handling `DONE`).
