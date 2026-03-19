# Patch 117 Spec Changes

These spec changes were present in `/Users/alec/Downloads/117.patch.txt` but were not applied to `spec/` directly.

They are captured here for later processing so the code changes can land without violating the repository rule against unrequested `spec/` edits.

## Summary

- Reframe real-time delivery around managed PR events instead of generic repository events.
- Replace the repo-scoped stream ADR with a user-scoped managed-PR stream ADR.
- Update architecture, data-flow, and daemon docs to describe one authenticated stream per Goddard user.
- Clarify that managed PR ownership is a Goddard concern and is distinct from GitHub author identity.

## `spec/README.md`

```diff
@@ -4,7 +4,7 @@
 
 Goddard unifies local repository workflows with GitHub operations and extends that bridge to autonomous AI execution.
 
-> Goddard provides a framework-agnostic TypeScript SDK, a Cloudflare-powered real-time backend, and a Tauri desktop workspace so developers and agents can create PRs, observe repository events, and act from shared local context.
+> Goddard provides a framework-agnostic TypeScript SDK, a Cloudflare-powered real-time backend, and a Tauri desktop workspace so developers and agents can create PRs, observe managed pull request events, and act from shared local context.
 
 ## The Problem
 
@@ -19,7 +19,7 @@ Goddard addresses:
 | # | Pillar | Meaning |
 |---|--------|---------|
 | 1 | SDK-first | Capabilities live in `@goddard-ai/sdk`; consumers stay thin. |
-| 2 | Real-time | Repository events stream to desktop workspaces and SDK consumers with low latency. |
+| 2 | Real-time | Managed pull request events stream to desktop workspaces and SDK consumers with low latency. |
 | 3 | Delegated identity | `goddard[bot]` acts on behalf of authenticated developers. |
 | 4 | Autonomous control | Built-in orchestration runs `pi-coding-agent` with safety limits. |
 | 5 | Type safety | APIs are TypeScript-first, while configuration remains machine-readable and validated at runtime. |
@@ -29,7 +29,7 @@ Goddard addresses:
 ## Usage Modes
 
 ### 1) SDK Integrations
-A developer or product integration uses `@goddard-ai/sdk` directly to authenticate, create pull requests, subscribe to repository events, and embed Goddard capabilities into custom hosts.
+A developer or product integration uses `@goddard-ai/sdk` directly to authenticate, create pull requests, subscribe to managed pull request events, and embed Goddard capabilities into custom hosts.
 
 ### 2) Desktop Workspace
 A human developer uses a unified, IDE-like desktop surface to monitor sessions, review pull requests, browse specs, and manage roadmap context without hopping across multiple tools.
@@ -37,7 +37,7 @@ A human developer uses a unified, IDE-like desktop surface to monitor sessions,
 ### 3) Background Automation
 A local runtime hosted by the desktop app or another SDK consumer handles unattended execution:
 - Loop mode for recurring `pi-coding-agent` cycles.
-- PR-feedback one-shot handling triggered by repository events.
+- PR-feedback one-shot handling triggered by managed pull request events.
 - Configurable limits for cadence, operations, and tokens.
 
 All modes consume the same SDK and backend authority model.
```

## `spec/adr/002-edge-native-backend.md`

```diff
@@ -7,18 +7,18 @@ ACTIVE
 
 The Goddard backend must handle two distinct workloads:
 1. **Stateless request handling** — auth, PR creation, webhook ingest. These are short-lived, independently scalable operations.
-2. **Stateful real-time fan-out** — maintaining open SSE connections per repository and broadcasting events to all subscribers. This requires per-repository state that survives across individual requests.
+2. **Stateful real-time fan-out** — maintaining open SSE connections for authenticated users and broadcasting managed PR events to the correct owner. This requires stream state that survives across individual requests.
 
 We needed a deployment model that serves both workloads with sub-second global latency, without managing servers.
 
 ## Decision
 
-The backend runs on **Cloudflare Workers** (stateless request handlers) with **Cloudflare Durable Objects** for per-repository SSE state and event fan-out. **Turso** (SQLite at the Edge) provides durable persistence for users, sessions, and GitHub App installations.
+The backend runs on **Cloudflare Workers** for stateless request handling and **Cloudflare Durable Objects** for user-scoped SSE state and event fan-out. **Turso** (SQLite at the Edge) provides durable persistence for users, sessions, GitHub App installations, and managed PR ownership.
 
 ## Rationale
 
 - **Global low latency:** Workers execute at the edge closest to the requester, satisfying the Real-Time pillar.
- **Durable Objects as session anchors:** Each `owner/repo` maps to a Durable Object instance that owns all SSE connections for that repo, providing strong isolation with no cross-repo interference.
+- **Durable Objects as stream anchors:** Each authenticated Goddard user can be mapped to a stream owner with isolated subscriber state and predictable fan-out behavior.
 - **No server management:** Cloudflare handles scaling, availability, and distribution.
 - **Turso complements the edge model:** SQLite at the edge avoids round-trips to a centralized database region.
 
@@ -26,4 +26,4 @@ The backend runs on **Cloudflare Workers** (stateless request handlers) with **C
 
 - Local development uses an in-memory control plane as a substitute for Turso and Durable Objects — this is a development convenience only, not a production mode.
 - Production deployment requires Cloudflare account setup, `wrangler.toml` configuration, and Turso credentials.
-- All backend logic must be compatible with the Workers runtime (no Node.js built-ins).
+- All backend logic must be compatible with the Workers runtime and must route real-time delivery by managed PR ownership rather than repository subscription lists.
```

## `spec/adr/004-sse-repo-stream.md`

```diff
deleted file mode 100644
--- a/spec/adr/004-sse-repo-stream.md
+++ /dev/null
@@ -1,27 +0,0 @@
-# ADR-004: Repository Stream Transport Uses SSE
-
-## Status
-ACTIVE
-
-## Context
-
-The original real-time stream transport used WebSockets. In practice, the stream path is server-to-client only: webhook and PR events are published by the backend and consumed by CLI clients. There is no requirement for bidirectional messaging over the same connection.
-
-Running interactive CLI consumers on Node also required extra WebSocket runtime handling and upgrade-specific server plumbing in local development.
-
-## Decision
-
-Repository event streaming uses **Server-Sent Events (SSE)** over standard HTTP (`text/event-stream`) instead of WebSocket upgrades.
-
-## Rationale
-
-- **Matches traffic shape:** The stream is one-way (server → client), which is SSE’s native model.
-- **Simpler infrastructure path:** SSE removes explicit HTTP upgrade handling in local Node adapters.
-- **Client portability:** SSE can be consumed with plain `fetch` stream parsing in the SDK, avoiding dependence on runtime WebSocket globals.
-- **Durable Object compatibility:** Per-repo fan-out remains anchored in Durable Objects with the same isolation model.
-
-## Consequences
-
-- SDK stream subscriptions now open a long-lived HTTP request and parse SSE frames.
-- Backend stream endpoints return `text/event-stream` responses and no longer rely on WebSocket `101` upgrades.
-- Existing consumers that directly depended on WebSocket semantics must migrate to the SDK stream API or SSE parsing.
```

## `spec/adr/005-user-scoped-managed-pr-stream.md`

```diff
new file mode 100644
--- /dev/null
+++ b/spec/adr/005-user-scoped-managed-pr-stream.md
@@ -0,0 +1,30 @@
+# ADR-005: Managed PR Event Delivery Uses User-Scoped SSE Streams
+
+## Status
+ACTIVE
+
+## Context
+
+The original stream model attached subscribers to repositories. That model no longer matched the product boundary for automation: managed PR feedback belongs to the authenticated Goddard user who initiated the managed PR, and a single daemon process may need feedback from many repositories at once.
+
+GitHub author identity is also not a reliable routing boundary. The backend already owns the managed-PR lifecycle, so it is the authoritative place to remember which Goddard user initiated a managed PR and should receive its later feedback events.
+
+## Decision
+
+Managed PR event delivery uses authenticated, user-scoped **Server-Sent Events (SSE)** streams.
+
+Each subscriber opens one long-lived stream for the current Goddard user. The backend routes PR-created events and later webhook feedback by managed-PR ownership. Repository membership alone does not determine delivery, and GitHub author identity does not override Goddard ownership.
+
+## Rationale
+
+- **Matches the automation actor:** Background automation is owned by an authenticated developer, not by a repository subscription list.
+- **Reduces client coordination:** SDK consumers and daemons maintain one stream instead of tracking repository-by-repository subscriptions.
+- **Preserves isolation:** User-scoped routing prevents managed PR feedback from leaking between Goddard users.
+- **Keeps the transport simple:** The stream remains one-way server-to-client traffic, so SSE continues to fit the delivery model.
+
+## Consequences
+
+- The backend must persist managed-PR ownership when a PR is created so later feedback can be routed correctly.
+- SDK, desktop, and daemon consumers subscribe once per authenticated user session rather than once per repository.
+- Unmanaged PRs are not delivered on the managed stream.
+- Delivery guarantees apply to managed PRs whose ownership was recorded under this routing model; older records outside that guarantee boundary are not promised stream delivery.
```

## `spec/core/architecture.md`

```diff
@@ -12,7 +12,7 @@
 | Distribution | `git-subrepo` to standalone repositories |
 
 ## Platform Components
- **Control Plane** — worker-hosted authority for sessions, managed PR state, and repository event fan-out.
+- **Control Plane** — worker-hosted authority for sessions, managed PR state, and user-scoped event fan-out.
 - **GitHub Integration** — delegated GitHub identity and webhook-facing integration behavior.
 - **SDK** — framework-agnostic platform client for programmatic and embedded hosts.
 - **Desktop Workspace** — Tauri desktop app and primary human-facing surface.
@@ -26,11 +26,12 @@ These components can be packaged independently and synchronized to standalone re
 - Session validation on protected requests.
 - Webhook ingest and routing for pull request and review feedback events.
 - Managed reaction behavior via GitHub App identity.
-- Per-repository event fan-out over SSE.
+- User-scoped event fan-out over SSE for managed PR ownership.
 
 Boundary:
 - Production persistence is Turso-backed.
 - Local in-memory mode is development-only convenience.
+- Real-time delivery follows authenticated managed-PR ownership rather than repository-scoped subscription state.
 
 ### SDK
 Design rule: platform capabilities live here first.
@@ -40,7 +41,7 @@ Design rule: platform capabilities live here first.
 
 ### Desktop Workspace
 - Primary human-facing workspace for authentication, session steering, PR review, specs, tasks, and roadmap context.
-- Use SDK contracts for pull request operations, stream subscription, and other platform interactions.
+- Use SDK contracts for pull request operations, managed-PR stream subscription, and other platform interactions.
 - Host or supervise local background automation when unattended execution is enabled.
 
 Boundary:
@@ -48,7 +49,7 @@ Boundary:
 - Must not fork platform behavior away from SDK contracts.
 
 ### Background Runtime
-- Subscribe to repository streams via SDK.
+- Subscribe to authenticated managed-PR streams via SDK.
 - Filter for managed PR feedback events.
 - Launch local one-shot `pi` sessions with PR context.
 - Operate as background automation rather than a user-facing command surface.
```

## `spec/core/data-flows.md`

```diff
@@ -5,13 +5,14 @@ This file captures conceptual end-to-end sequences only. Wire formats and API pa
 ## PR Creation (User-Initiated)
 
 1. Developer initiates PR creation from the desktop app or an SDK-powered host.
-2. SDK validates intent and forwards request through platform contracts.
-3. Backend validates session and resolves GitHub identity.
-4. Backend creates PR via GitHub App delegation and records managed-PR metadata.
-5. Reviewer responds on GitHub (comment/review).
-6. Webhook event enters backend and is routed to repository stream state.
-7. Stream broadcast reaches subscribed clients.
-8. SDK normalizes the event; the desktop app or host updates UI and local state.
+2. SDK validates intent and forwards the request through platform contracts.
+3. Backend validates the session, resolves GitHub identity, and records which Goddard user owns the managed PR.
+4. Backend creates the PR through delegated GitHub authority and persists enough managed-PR identity to pair later feedback with the owning Goddard user.
+5. Reviewer responds on GitHub with comments or review feedback.
+6. Webhook event enters the backend.
+7. Backend determines whether the referenced PR is managed and, if so, which Goddard user owns it.
+8. Event delivery is routed onto that user's authenticated stream.
+9. SDK normalizes the event; the desktop app or host updates UI and local state.
 
 ## Authentication (Lazy Device Flow)
 
@@ -24,11 +25,12 @@ This file captures conceptual end-to-end sequences only. Wire formats and API pa
 
 ## Real-Time Event Subscription (Background Runtime)
 
-1. Desktop app or background runtime subscribes to repository stream via SDK.
-2. Backend validates session and attaches the subscriber connection to repo stream.
-3. GitHub webhook events are routed to that repo stream.
-4. SDK emits typed feedback events.
-5. Subscriber updates workspace state or may launch one-shot `pi` execution.
+1. Desktop app or background runtime subscribes to an authenticated managed-PR event stream via SDK.
+2. Backend validates the session and attaches the subscriber connection to the current Goddard user's stream.
+3. Managed-PR events owned by that user may arrive from multiple repositories over the same stream.
+4. Unmanaged PR events and events owned by other Goddard users are not delivered on that stream.
+5. SDK emits typed feedback events.
+6. Subscriber updates workspace state or may launch one-shot `pi` execution.
 
 ## Autonomous Cycle (Loop)
```

## `spec/daemon.md`

```diff
@@ -2,43 +2,44 @@
 
 ## Goal
 
-Use real-time repository feedback to trigger focused, local one-shot `pi` sessions without requiring a human to monitor a live event feed.
+Use real-time managed-PR feedback to trigger focused, local one-shot `pi` sessions without requiring a human to monitor a live event feed.
 
 ## Hypothesis
 
-We believe that immediate, automated handling of managed-PR comments/reviews will reduce reviewer wait time and improve PR throughput.
+We believe that immediate, automated handling of managed-PR comments and reviews will reduce reviewer wait time and improve PR throughput.
 
 ## Actors
 
 - **Local Runtime Host** — desktop app-managed background worker or another supervised local process with repository access.
+- **Authenticated Goddard User** — the developer identity that owns the daemon's stream and the managed PRs routed onto it.
 - **Reviewer** — submits PR comments or reviews on GitHub.
 - **Goddard GitHub App** — origin of managed PR metadata and webhook events.
 
 ## State Model
 
-`Idle -> Subscribed -> EventReceived -> EligibilityChecked -> OneShotQueued -> OneShotRunning -> OneShotCompleted -> Idle`
+`Idle -> Connected -> EventReceived -> EligibilityChecked -> OneShotQueued -> OneShotRunning -> OneShotCompleted -> Connected`
 
 ## Core Behavior
 
-1. Background runtime subscribes to repository stream events through SDK.
-2. On feedback events, runtime checks whether PR is Goddard-managed.
-3. Eligible events enqueue one one-shot task per PR.
-4. Task launches local `pi` with repository, PR number, and reviewer feedback context.
-5. Prompt contract requires the session to conclude by posting a PR-thread response.
-6. Runtime returns to subscribed mode and continues event processing.
+- Each daemon process maintains one authenticated event stream for the current Goddard user.
+- That stream may carry managed-PR feedback from multiple repositories when those PRs are owned by the current Goddard user.
+- The runtime evaluates incoming events for one-shot eligibility and queues work by pull request, never by repository subscription boundaries.
+- One-shot execution always uses the repository and pull request context carried by the event.
+- After each one-shot completes, the runtime returns to connected listening mode.
 
 ## Hard Constraints
 
- Trigger only on PR comment/review feedback events.
- Ignore non-managed PRs.
+- Trigger only on PR comment and review feedback events.
+- Consume a single authenticated stream per daemon process.
+- React only to managed PRs owned by the authenticated Goddard user.
 - Avoid overlapping one-shot execution for the same PR.
- Continue running until interrupted by host supervisor.
+- Continue running until interrupted by the host supervisor.
 
 ## Failure Handling Expectations
 
 - Stream disconnects should trigger reconnect attempts with bounded backoff.
 - One-shot launch failures must be logged with PR context and must not crash the runtime.
-- If multiple events arrive while a PR task is active, the runtime should coalesce or queue by PR (never run concurrently for the same PR).
+- If multiple events arrive while a PR task is active, the runtime should coalesce or queue by PR and never run concurrently for the same PR.
 
 ## Non-Goals
 
@@ -48,4 +49,4 @@ We believe that immediate, automated handling of managed-PR comments/reviews wil
 
 ## Decision Memory
 
-Pivoted from a human-facing live stream viewer to daemon ownership because stream events are operational automation triggers, not primarily interactive output.
+The daemon originally followed repository-scoped streams. That model no longer matched the actual ownership boundary for managed PR automation, so the daemon now follows the authenticated Goddard user and consumes one unified stream across repositories.
```
