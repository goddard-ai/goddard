# Project Goddard: Architecture & Implementation Proposal

## 1. Executive Summary
Goddard is a modern, real-time developer tooling suite designed to bridge local terminal workflows with GitHub operations. It allows developers to create Pull Requests, trigger GitHub Actions, and receive real-time streaming updates about repository events directly in their terminal.

To ensure maximum ecosystem compatibility, the core logic is packaged as a framework-agnostic TypeScript SDK, which powers a thin CLI wrapper. The backend leverages edge computing (Cloudflare Workers) and edge databases (Turso) for low-latency, globally distributed performance.

## 2. Key Capabilities
*   **Delegated PR Creation:** The CLI creates PRs via the GitHub App identity (`goddard[bot]`), while explicitly mentioning the authenticated human developer responsible for the action.
*   **Real-Time Terminal Streaming:** The CLI subscribes to repository events (comments, reviews) via WebSockets, streaming them directly into the developer's terminal in real-time.
*   **Automated Reactions:** The GitHub App automatically reacts (e.g., 👀 emoji) to comments/reviews on PRs it manages.
*   **Action Triggers:** Developers can trigger GitHub Actions workflows programmatically or via the CLI.
*   **SDK-First Design:** All capabilities are exposed via a TypeScript SDK, allowing third parties to build custom automations, bots, or GUIs without relying on CLI subprocesses.

---

## 3. Technology Stack
*   **Infrastructure / API:** Cloudflare Workers (handles REST API, Webhooks, OAuth, and WebSockets via Durable Objects).
*   **Database:** Turso (SQLite at the Edge) with Drizzle ORM.
*   **Authentication:** GitHub OAuth Device Flow (CLI login).
*   **Package Management:** `pnpm` Workspaces.
*   **Source Control:** Monorepo with `git-subrepo` for standalone package distribution.

---

## 4. Repository Architecture
The codebase uses a monorepo approach for streamlined local development, while utilizing `git-subrepo` to publish standalone, cleanly-versioned repositories for open-source consumers.

```text
goddard/                     # Monorepo Root
├── backend/                 # Cloudflare Worker, Durable Objects, Drizzle Schema
│   └── .gitrepo             -> Syncs to github.com/org/goddard-backend
├── cmd/                     # Thin CLI wrapper (commander/oclif)
│   └── .gitrepo             -> Syncs to github.com/org/goddard-cli
├── github-app/              # App manifest, webhook handlers, Octokit logic
│   └── .gitrepo             -> Syncs to github.com/org/goddard-github-app
├── sdk/                     # Framework-agnostic TypeScript client library
│   └── .gitrepo             -> Syncs to github.com/org/goddard-sdk
└── pnpm-workspace.yaml      # Manages internal cross-dependencies
```

### Automated Synchronization
A GitHub Action running on the `main` branch of the monorepo automatically isolates commits for each sub-directory and pushes them to their respective external repositories using `git-subrepo`. This ensures consumers can clone/install standalone packages without monorepo noise.

---

## 5. Component Deep Dive

### A. The Backend (`backend/` & `github-app/`)
Hosted on Cloudflare Workers, this acts as the control plane.
*   **Database Schema (Turso/Drizzle):**
    *   `users`: Maps GitHub IDs to usernames.
    *   `cli_sessions`: Stores Device Flow verification state.
    *   `installations`: Maps GitHub repository names (`owner/repo`) to App Installation IDs.
*   **Authentication Flow:** Implements GitHub's Device Flow. The CLI requests a code, the user authorizes via browser, and the backend issues a session token.
*   **Webhook Handling:** Receives `pull_request`, `issue_comment`, and `pull_request_review` events from GitHub. Uses Octokit to apply automated reactions.
*   **Real-Time Streaming:** Utilizes **Cloudflare Durable Objects**. Webhook events are routed to a repository-specific Durable Object, which then broadcasts the event down open WebSockets connected to authenticated CLI clients.

### B. The Core SDK (`sdk/`)
A framework-agnostic TypeScript library (`@goddard/sdk`). It contains zero assumptions about its environment (browser, Node, Cloudflare).
*   **Dependency Injection:** Accepts a `TokenStorage` interface to allow flexibility in how session tokens are stored (in-memory, file system, or `localStorage`).
*   **Modules:**
    *   `sdk.pr.create()`: Sends intents to the backend.
    *   `sdk.actions.trigger()`: Dispatches workflows.
    *   `sdk.stream.subscribeToRepo()`: Manages the WebSocket connection and normalizes incoming frames into standard `EventEmitter` events (e.g., `stream.on('comment', handler)`).

### C. The CLI (`cmd/`)
A thin UI wrapper around the SDK (`@goddard/cli`).
*   **Context Inference:** Automatically parses local `.git/config` to determine the current `owner/repo`.
*   **Storage:** Implements the SDK's `TokenStorage` interface using the local file system (e.g., `~/.goddard/config.json`).
*   **User Experience:** Provides terminal spinners, colored output, and formatting for the real-time event stream.

---

## 6. Data & Execution Flow Example (Creating a PR)
1.  **Dev invokes CLI:** `goddard pr create -m "Fix memory leak"`
2.  **SDK executes:** The CLI formats the request and passes it to `@goddard/sdk`.
3.  **Backend validates:** The SDK calls the Cloudflare Worker API. The Worker checks the Turso DB to validate the session and fetch the user's GitHub identity.
4.  **App acts:** The Worker uses the `goddard[bot]` installation token to create the PR via the GitHub API, appending `Authored via CLI by @username` to the PR body.
5.  **Review happens:** A coworker comments on the PR on GitHub.com.
6.  **Webhook fires:** GitHub POSTs the comment event to the Worker.
7.  **Bot reacts:** The Worker immediately uses Octokit to add an 👀 reaction to the comment.
8.  **Stream broadcasts:** The Worker passes the payload to the Durable Object, which pushes it down the WebSocket.
9.  **Terminal updates:** The SDK parses the WebSocket frame, emits a `comment` event, and the CLI prints the comment natively in the developer's terminal.

---

## 7. Current Build Status (Local MVP)

Implemented in this repository:
- SDK-first architecture (`sdk/`) used by CLI (`cmd/`) and integrations.
- Local backend control plane with auth flow endpoints, PR creation, action trigger, webhook ingest, and WebSocket repo streams.
- GitHub App shim (`github-app/`) that forwards webhook events to backend.
- Monorepo CI and subrepo sync workflow scaffolding.

Latest hardening pass:
- Added auth/session expiration checks in backend.
- Added request body size limits and explicit invalid JSON handling.
- Added SDK stream payload guards so malformed frames emit `error` instead of crashing listeners.
- Added tests for session expiry, invalid JSON handling, and malformed stream payloads.

## 8. Path to Production Deployment

While the local MVP is fully functional and tested, the following work is required to transition to the production architecture described in sections 3-5:

### A. Persistence & Infrastructure (The "Control Plane")
*   **Database Migration:** Replace the `InMemoryBackendControlPlane` with a production implementation using **Turso** (SQLite at the edge) and **Drizzle ORM**.
    *   Define schema for `users`, `auth_sessions`, `pull_requests`, and `action_runs`.
    *   Implement migration scripts for Turso.
*   **Cloudflare Workers Deployment:**
    *   Port the `backend` package to the Cloudflare Workers runtime.
    *   Replace the in-memory WebSocket management with **Cloudflare Durable Objects** for scalable, real-time broadcasting.
    *   Add `wrangler.toml` for deployment configuration.
*   **Production Secrets:**
    *   Provision and configure `TURSO_DB_URL` and `TURSO_DB_AUTH_TOKEN` in the production environment.
    *   Configure GitHub App credentials (`GITHUB_APP_ID`, `GITHUB_PRIVATE_KEY`) for real PR creation and action triggering.

### B. Monorepo Distribution (`git-subrepo`)
*   **External Repository Provisioning:** Create the four standalone repositories on GitHub (e.g., `goddard-sdk`, `goddard-cli`, etc.).
*   **Subrepo Initialization:**
    *   Run `git subrepo init` for `backend/`, `cmd/`, `github-app/`, and `sdk/` to create their `.gitrepo` metadata.
    *   Verify push/pull permissions between the monorepo and standalone targets.
*   **CI/CD Automation:**
    *   Configure the `SYNC_PAT` (Personal Access Token) as a repository secret in the monorepo.
    *   Activate and verify the `.github/workflows/sync-subrepos.yml` workflow on the `main` branch.

### C. Developer Experience (Local Dev)
*   **Local Persistence Support:** Add a local SQLite/Drizzle mode for developers who want to test persistence without a Turso account.
*   **Environment Validation:** Add a pre-flight check to the CLI/Backend to ensure all required environment variables are present before starting.

Status note: Local MVP runtime is deployable today (backend + CLI + SDK + webhook bridge) for transient testing. Production persistence and automated subrepo publishing require the infrastructure and metadata setup described above.