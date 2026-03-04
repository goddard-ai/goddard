---
id: system-architecture
status: ACTIVE
links:
  - type: Extends
    target: spec/vision.md
  - type: Relates-To
    target: spec/data-flows.md
  - type: Relates-To
    target: spec/adr/001-sdk-first-architecture.md
  - type: Relates-To
    target: spec/adr/002-edge-native-backend.md
  - type: Relates-To
    target: spec/adr/004-sse-repo-stream.md
  - type: Relates-To
    target: spec/daemon/index.md
---

# Architecture

## Technology Stack
| Layer | Technology |
|-------|-----------|
| API / Webhooks / SSE | Cloudflare Workers |
| Real-time broadcast | Cloudflare Durable Objects |
| Database | Turso (SQLite at the Edge) + Drizzle ORM |
| Authentication | GitHub OAuth Device Flow |
| Package management | `pnpm` Workspaces |
| Distribution | `git-subrepo` to standalone repositories |

## Repository Topology
Monorepo packages:
- `backend/` — Worker control plane + Durable Object routing.
- `github-app/` — GitHub App integration and webhook-facing behavior.
- `sdk/` — framework-agnostic platform client (`@goddard-ai/sdk`).
- `cmd/` — terminal UX for interactive and loop commands.
- `daemon/` — stream consumer for PR-feedback one-shot automation.

Each package can also be published as a standalone repository via subrepo sync.

## Component Responsibilities
### Backend (`backend/` + `github-app/`)
- Device Flow state management and session issuance.
- Session validation on protected requests.
- Webhook ingest and routing (`pull_request`, `issue_comment`, `pull_request_review`).
- Managed reaction behavior via GitHub App identity.
- Per-repo event fan-out over WebSocket through Durable Objects.

Boundary:
- Production persistence is Turso-backed.
- Local in-memory mode is development-only convenience.

### SDK (`sdk/`)
Design rule: platform capabilities live here first.
- Expose typed operations for PR creation and stream subscription.
- Normalize stream frames into stable event contracts.
- Accept injected `TokenStorage` to avoid environment lock-in.

See [`adr/001-sdk-first-architecture.md`](./adr/001-sdk-first-architecture.md).

### Interactive CLI (`cmd/`)
- Human terminal UX and command parsing.
- Local token persistence.
- Repo inference from `.git/config` when possible.

See [`cli/interactive.md`](./cli/interactive.md).

### Feedback Daemon (`daemon/`)
- Subscribe to repo streams via SDK.
- Filter for managed PR feedback events.
- Launch local one-shot `pi` sessions with PR context.

See [`daemon/index.md`](./daemon/index.md).

### Autonomous Loop (`cmd/` loop)
- Config discovery and validation handoff.
- Loop runtime instantiation.
- `systemd` unit generation for supervised deployments.

See [`cli/loop.md`](./cli/loop.md), [`runtime-loop.md`](./runtime-loop.md), and [`configuration.md`](./configuration.md).

## Deployment Model
Target runtime is Cloudflare Workers with Durable Objects for per-repo fan-out.

Production prerequisites:
- Turso database.
- Registered GitHub App with webhook delivery.
- Durable Objects namespace binding.
- Secret management through Cloudflare.

See [`adr/002-edge-native-backend.md`](./adr/002-edge-native-backend.md).
