# Architecture

## Technology Stack
| Layer | Technology |
|-------|-----------|
| API / Webhooks / SSE | Cloudflare Workers |
| Real-time broadcast | Server-Sent Events (SSE) / Cloudflare Workers |
| Database | Turso (SQLite at the Edge) |
| Authentication | GitHub OAuth Device Flow |
| Desktop application | Trusted desktop host + web frontend |

## Platform Components
- **Control Plane** — worker-hosted authority for sessions, managed pull request state, and user-scoped event fan-out.
- **GitHub Integration** — delegated GitHub identity and webhook-facing integration behavior.
- **SDK** — framework-agnostic daemon control-plane client for programmatic and embedded hosts.
- **Desktop Workspace** — app and primary human-facing surface.
- **Background Runtime** — supervised automation host for unattended execution, including daemon runtimes where appropriate.
- **Operational CLI** — thin terminal control surface for initializing or controlling local automation without becoming a parallel primary UX.

## Component Responsibilities
### Control Plane
- Device Flow state management and session issuance.
- Session validation on protected requests.
- Webhook ingest and routing for pull request and review feedback events.
- Managed reaction behavior via GitHub App identity.
- User-scoped event fan-out over SSE for managed ownership.

Boundaries:
- Production persistence is Turso-backed.
- Local in-memory mode is development-only convenience.
- Real-time delivery follows authenticated ownership rather than repository-scoped subscription state.

### SDK
Design rule: daemon control capabilities live here first.
- Expose typed operations for daemon authentication and automation control.
- Serve as the thin programmatic control plane for daemon behavior rather than as a general real-time backend client.
- Keep backend auth state out of SDK-owned persistence and route user auth through the daemon boundary.

### Desktop Workspace
- Primary human-facing workspace for authentication, session steering, pull request review, specs, tasks, and roadmap context.
- Use SDK contracts for daemon authentication and other platform interactions.
- Host or supervise local background automation when unattended execution is enabled.

Boundaries:
- Must keep privileged OS and daemon access behind the trusted desktop host boundary.
- Embedded browser surfaces must not bypass that boundary for direct daemon access.
- Must not fork platform behavior away from SDK contracts.

### Background Runtime
- Own authenticated event stream consumption as part of supervised automation behavior.
- Launch PR feedback flows for managed feedback.
- Host or cooperate with workforce orchestration for repository delegation.
- Operate as background automation rather than a user-facing command surface.
- Be hostable by the app or another supervised local process when needed.

### Operational CLI
- Initialize repository-local automation intent when a local filesystem touchpoint is required.
- Start, inspect, and mutate local automation as a thin operator surface.
- Reuse SDK and daemon contracts rather than reimplementing runtime ownership.

Boundaries:
- Must not become the primary human-facing workspace.
- Must not create a parallel platform contract outside the SDK and daemon authority model.

## Deployment Model
The control plane runs on Cloudflare Workers. The primary human-facing local runtime is the app. Unattended automation may be hosted by the app or by another supervised local process when needed, with daemon runtimes available for supported automation domains.

Production prerequisites:
- Turso database.
- Registered GitHub App with webhook delivery.
- Secret management through Cloudflare.
