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
- **Control Plane** — worker-hosted authority for sessions, managed pull request state, and user-scoped event fan-out.
- **GitHub Integration** — delegated GitHub identity and webhook-facing integration behavior.
- **SDK** — framework-agnostic daemon control-plane client for programmatic and embedded hosts.
- **Desktop Workspace** — Tauri desktop app and primary human-facing surface.
- **Background Runtime** — supervised local automation host for unattended execution, including daemon-managed runtimes where appropriate.
- **Operational CLI** — thin terminal control surface for initializing or controlling daemon-backed local automation without becoming a parallel primary UX.

These components can be packaged independently and synchronized to standalone repositories when distribution needs require it.

## Component Responsibilities
### Control Plane
- Device Flow state management and session issuance.
- Session validation on protected requests.
- Webhook ingest and routing for pull request and review feedback events.
- Managed reaction behavior via GitHub App identity.
- User-scoped event fan-out over SSE for managed pull request ownership.

Boundary:
- Production persistence is Turso-backed.
- Local in-memory mode is development-only convenience.
- Real-time delivery follows authenticated managed pull request ownership rather than repository-scoped subscription state.

### SDK
Design rule: daemon control capabilities live here first.
- Expose typed operations for daemon-backed authentication and daemon-backed local automation control.
- Serve as the thin programmatic control plane for daemon-managed local behavior rather than as a general real-time backend client.
- Keep backend auth state out of SDK-owned persistence and route user auth through the daemon boundary.

### Desktop Workspace
- Primary human-facing workspace for authentication, session steering, pull request review, specs, tasks, and roadmap context.
- Use SDK contracts for daemon-backed authentication and other platform interactions.
- Host or supervise local background automation when unattended execution is enabled.

Boundary:
- Must remain Tauri-first and rely on official plugins for OS integrations.
- Must not fork platform behavior away from SDK contracts.

### Background Runtime
- Own authenticated managed pull request stream consumption as part of supervised local automation behavior.
- Launch daemon-managed PR feedback flows for managed pull request feedback.
- Host or cooperate with daemon-managed workforce orchestration for repository-scoped delegation.
- Operate as background automation rather than a user-facing command surface.
- Be hostable by the desktop workspace or another supervised local process when needed.

### Operational CLI
- Initialize repository-local automation intent when a local filesystem touchpoint is required.
- Start, inspect, and mutate daemon-backed local automation as a thin operator surface.
- Reuse SDK and daemon contracts rather than reimplementing runtime ownership.

Boundary:
- Must not become the primary human-facing Goddard workspace.
- Must not create a parallel platform contract outside the SDK and daemon authority model.

## Deployment Model
The control plane runs on Cloudflare Workers. The primary human-facing local runtime is the Tauri desktop workspace. Unattended automation may be hosted by the desktop workspace or by another supervised local process when needed, with daemon-managed local runtimes available for supported automation domains.

Production prerequisites:
- Turso database.
- Registered GitHub App with webhook delivery.
- Secret management through Cloudflare.
