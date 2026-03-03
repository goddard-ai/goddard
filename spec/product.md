# Product Specification

## Primary Users

| User type | Description |
|-----------|-------------|
| **Interactive developer** | Runs `goddard` CLI to create PRs and watch repository events from the terminal. |
| **Operator** | Configures and supervises a long-running autonomous agent loop against a codebase. |

---

## Core Jobs-to-Be-Done

### Interactive mode
1. Authenticate with GitHub once via Device Flow.
2. Create PRs without leaving the terminal, attributed to the real developer.
3. Receive real-time repository event notifications (comments, reviews) while working.
4. Trigger GitHub Actions workflows programmatically.

### Autonomous mode
1. Initialize a typed loop config quickly (`goddard loop init`).
2. Run autonomous agent cycles indefinitely with bounded operational behavior.
3. Adjust prompt strategy without modifying loop internals.
4. Control cadence and operation rate to avoid excessive spend or load.
5. Deploy as a `systemd` service in production environments.

---

## Key User Outcomes

- `goddard login` completes in under 60 seconds with no manual token management.
- `goddard stream --repo owner/repo` begins streaming events within 2 seconds of a GitHub event.
- `goddard loop init` produces a valid, runnable `goddard.config.ts` immediately.
- `goddard loop run` discovers and loads local config automatically.
- `goddard loop generate-systemd` emits a ready-to-use `.service` file.
- All commands emit deterministic, human-readable errors for missing or invalid config.

---

## MVP Success Criteria

### Platform (goddard)
- CLI commands: `login`, `whoami`, `pr create`, `actions trigger`, `stream`.
- SDK exports: `createGoddardSdk(config)`, `TokenStorage` interface.
- Backend: auth, PR creation, webhook handling, WebSocket broadcast.
- GitHub App: automated reactions on managed PRs.

### Agent Orchestration (loop)
- CLI commands: `goddard loop init`, `goddard loop run`, `goddard loop generate-systemd`.
- Public API exports `createGoddardLoop` and `createGoddardConfig`.
- Strategy contract supports custom classes implementing `nextPrompt(ctx)`.
- Runtime performs repeated cycles and carries forward prior summary context.

---

## Planned Evolution (not guaranteed by current runtime)

- Stricter token-budget enforcement with configurable hard-stop vs. warn behavior.
- Richer metrics and observability (Prometheus exporter).
- Stronger model/config bridging between loop config and `pi-coding-agent` runtime.
- Explicit cycle termination protocol (handling `DONE` signal from agent output).
- Local SQLite/Drizzle mode for offline-capable persistence.
- Pre-flight environment validation before loop or backend startup.
