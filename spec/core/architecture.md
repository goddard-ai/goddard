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

## Platform Components
- **Control Plane** — worker-hosted authority for sessions, managed PR state, and user-scoped event fan-out.
- **GitHub Integration** — delegated GitHub identity and webhook-facing integration behavior.
- **SDK** — framework-agnostic platform client for programmatic and embedded hosts.
- **Desktop Workspace** — Tauri desktop app and primary human-facing surface.
- **Background Runtime** — supervised local automation host for unattended PR-feedback reactions and loop execution when needed.

These components can be packaged independently and synchronized to standalone repositories when distribution needs require it.

## Component Responsibilities
### Control Plane
- Device Flow state management and session issuance.
- Session validation on protected requests.
- Webhook ingest and routing for pull request and review feedback events.
- Managed reaction behavior via GitHub App identity.
- User-scoped event fan-out over SSE for managed PR ownership.

Boundary:
- Production persistence is Turso-backed.
- Local in-memory mode is development-only convenience.
- Real-time delivery follows authenticated managed-PR ownership rather than repository-scoped subscription state.

### SDK
Design rule: platform capabilities live here first.
- Expose typed operations for authentication, PR creation, and stream subscription.
- Normalize stream frames into stable event contracts.
- Accept injected `TokenStorage` to avoid environment lock-in.

### Desktop Workspace
- Primary human-facing workspace for authentication, session steering, PR review, specs, tasks, and roadmap context.
- Use SDK contracts for pull request operations, managed-PR stream subscription, and other platform interactions.
- Host or supervise local background automation when unattended execution is enabled.

Boundary:
- Must remain Tauri-first and rely on official plugins for OS integrations.
- Must not fork platform behavior away from SDK contracts.

### Background Runtime
- Subscribe to authenticated managed-PR streams via SDK.
- Filter for managed PR feedback events.
- Launch local one-shot `pi` sessions with PR context.
- Operate as background automation rather than a user-facing command surface.
- Be hostable by the desktop workspace or another SDK-based local supervisor.

## Deployment Model
The control plane runs on Cloudflare Workers. The primary human-facing local runtime is the Tauri desktop workspace. Unattended automation may be hosted by the desktop workspace or by another SDK-based supervisor when needed.

Production prerequisites:
- Turso database.
- Registered GitHub App with webhook delivery.
- Secret management through Cloudflare.
