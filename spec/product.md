---
id: product-specification
status: ACTIVE
links:
  - type: Extends
    target: spec/vision.md
  - type: Relates-To
    target: spec/non-goals.md
  - type: Relates-To
    target: spec/daemon/pr-feedback-one-shot.md
---

# Product Specification

## Primary Personas

| Persona | Description |
|---------|-------------|
| **Interactive Developer** | Runs `goddard` CLI to authenticate, create PRs, and drive focused AI sessions from the terminal. |
| **Operator** | Configures and supervises autonomous runtimes (loop and daemon) against a codebase. |

---

## Core Jobs-to-Be-Done

### Interactive Developer
1. Authenticate with GitHub once via Device Flow — no manual token management.
2. Create PRs without leaving the terminal, attributed to the real developer via `goddard[bot]`.
3. Trigger GitHub Actions workflows programmatically.
4. Launch focused local AI sessions (`spec`, `propose`) when needed.

### Operator
1. Initialize a typed loop config quickly (`goddard loop init`).
2. Run autonomous agent cycles indefinitely with bounded operational behavior.
3. Run a daemon that listens for PR feedback and triggers one-shot AI sessions.
4. Adjust prompt strategy without modifying runtime internals.
5. Control cadence and operation rate to avoid excessive spend or load.
6. Deploy as a `systemd` service in production environments.

---

## Key User Outcomes

- `goddard login` completes in under 60 seconds with no manual token management.
- `goddard-daemon run --repo owner/repo` receives and reacts to feedback events within 2 seconds of webhook ingest.
- `goddard loop init` produces a valid, runnable `goddard.config.ts` immediately.
- `goddard loop run` discovers and loads local config automatically without a pre-compile step.
- `goddard loop generate-systemd` emits a ready-to-use `.service` file.
- All commands emit deterministic, human-readable errors for missing or invalid config.

---

## MVP Success Criteria

### Platform (goddard)
- CLI commands: `login`, `logout`, `whoami`, `pr create`, `spec`, `propose`, `agents init`.
- Daemon command: `goddard-daemon run`.
- SDK exports: `createSdk(config)`, `TokenStorage` interface.
- Backend: auth, PR creation, managed-PR lookup, webhook handling, SSE broadcast.
- GitHub App: automated reactions on managed PRs.

### Agent Orchestration (loop)
- CLI commands: `goddard loop init`, `goddard loop run`, `goddard loop generate-systemd`.
- Public API exports `createGoddardLoop` and `createGoddardConfig`.
- Strategy contract supports custom classes implementing `nextPrompt(ctx)`.
- Runtime performs repeated cycles and carries forward prior summary context.

---

## Planned Evolution

The following are desirable future directions. They are not guaranteed by the current runtime and require explicit prioritization before implementation.

- **Stricter token-budget enforcement** — configurable hard-stop vs. warn behavior.
- **Richer observability** — Prometheus exporter backing the `metrics.prometheusPort` config surface.
- **Cycle termination protocol** — formal handling of `DONE` variants and structured agent-to-loop signaling.
- **Offline-capable persistence** — local SQLite/Drizzle mode without Turso.
- **Pre-flight validation** — explicit environment and config checks before loop or backend startup.
- **Model/config bridging** — stronger integration between loop config and `pi-coding-agent` model/provider settings.
