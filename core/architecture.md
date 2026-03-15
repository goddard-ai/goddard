# Architecture

## Technology Stack
| Layer | Technology |
|-------|-----------|
| API / Webhooks / SSE | Cloudflare Workers |
| Real-time broadcast | Server-Sent Events (SSE) / Cloudflare Workers |
| Database | Turso (SQLite at the Edge) + Drizzle ORM |
| Authentication | GitHub OAuth Device Flow |
| Desktop application | Tauri + web frontend |
| Package management | `pnpm` Workspaces |
| Distribution | `git-subrepo` to standalone repositories |

## Repository Topology
Monorepo packages:
- `backend/` — Worker control plane.
- `github-app/` — GitHub App integration and webhook-facing behavior.
- `sdk/` — framework-agnostic platform client (`@goddard-ai/sdk`).
- `app/` — Tauri desktop workspace and primary human-facing surface.
- `daemon/` — background automation runtime for PR-feedback one-shot handling when local supervision is required.

Each package can also be published as a standalone repository via subrepo sync.

## Component Responsibilities
### Backend (`backend/` + `github-app/`)
- Device Flow state management and session issuance.
- Session validation on protected requests.
- Webhook ingest and routing (`pull_request`, `issue_comment`, `pull_request_review`).
- Managed reaction behavior via GitHub App identity.
- Per-repo event fan-out over SSE.

Boundary:
- Production persistence is Turso-backed.
- Local in-memory mode is development-only convenience.

### SDK (`sdk/`)
Design rule: platform capabilities live here first.
- Expose typed operations for authentication, PR creation, and stream subscription.
- Normalize stream frames into stable event contracts.
- Accept injected `TokenStorage` to avoid environment lock-in.

### Desktop App (`app/`)
- Primary human-facing workspace for authentication, session steering, PR review, specs, tasks, and roadmap context.
- Use SDK contracts for pull request operations, stream subscription, and other platform interactions.
- Host or supervise local background automation when unattended execution is enabled.

Boundary:
- Must remain Tauri-first and rely on official plugins for OS integrations.
- Must not fork platform behavior away from SDK contracts.

### Feedback Daemon (`daemon/`)
- Subscribe to repo streams via SDK.
- Filter for managed PR feedback events.
- Launch local one-shot `pi` sessions with PR context.
- Operate as background automation rather than a user-facing command surface.

## Deployment Model
Target control-plane runtime is Cloudflare Workers. Primary local human-facing runtime is the Tauri desktop app.

Production prerequisites:
- Turso database.
- Registered GitHub App with webhook delivery.
- Secret management through Cloudflare.
