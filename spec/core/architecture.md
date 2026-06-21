# Architecture

## Technology Stack
| Layer | Technology |
|-------|-----------|
| API / Webhooks / SSE | Cloudflare Workers |
| Real-time broadcast | Server-Sent Events (SSE) / Cloudflare Workers |
| Database | Turso (SQLite at the Edge) |
| Authentication | GitHub OAuth Device Flow |
| Desktop application | Trusted desktop host + web frontend |
| Browser-to-local daemon access | Loopback HTTP with explicit origin validation and bearer-token authorization |

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
- Bootstrap its embedded webview with short-lived daemon-issued webview tokens when the webview connects directly to daemon loopback IPC.

Boundaries:
- Must keep privileged OS access behind the trusted desktop host boundary.
- Embedded browser daemon access must be host-mediated even when requests go directly from the webview to daemon loopback IPC: the trusted host obtains the webview token, the token is scoped to the current app/webview session, and the daemon still validates the request origin.
- Must not fork platform behavior away from SDK contracts.

### Browser Workspace
- A hosted browser surface may present the workspace UI and connect to a local daemon over loopback HTTP when local browser access is explicitly enabled.
- Hosted browser access uses local daemon pairing, an origin-bound bearer token, explicit origin allowlisting, host validation, and Private Network Access-compatible preflight behavior.

Boundaries:
- Browser access is disabled by default and must fail closed for missing, malformed, unconfigured, or ambiguous origins.
- `https://app.goddardai.org` is the initial production browser origin; any other hosted or local development origin requires explicit daemon configuration.
- Browser access must not use cookies for local daemon authorization and must not rely on CORS as authorization.
- Discovery may use an explicit connection URL, custom protocol, or manual port entry, but must not silently perform broad port scanning.

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
