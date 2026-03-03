# Architecture & Implementation

## Technology Stack
*   **Infrastructure / API:** Cloudflare Workers (handles REST API, Webhooks, OAuth, and WebSockets via Durable Objects).
*   **Database:** Turso (SQLite at the Edge) with Drizzle ORM.
*   **Authentication:** GitHub OAuth Device Flow (CLI login).
*   **Package Management:** `pnpm` Workspaces.
*   **Source Control:** Monorepo with `git-subrepo` for standalone package distribution.

---

## Repository Architecture
The codebase uses a monorepo approach for streamlined local development, while utilizing `git-subrepo` to publish standalone, cleanly-versioned repositories for open-source consumers.

```text
goddard/                     # Monorepo Root
├── backend/                 # Cloudflare Worker, Durable Objects, Drizzle Schema
│   └── .gitrepo             -> Syncs to github.com/org/goddard-backend
├── cmd/                     # Thin CLI wrapper (commander/oclif) for Goddard & pi-loop
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

## Component Deep Dive

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
    *   `sdk.stream.subscribeToRepo()`: Manages the WebSocket connection and normalizes incoming frames into standard `EventEmitter` events (e.g., `stream.on('comment', handler)`).

### C. The CLI (`cmd/`)
A thin UI wrapper around the SDK (`@goddard/cli`) that includes `pi-loop` automation.
*   **Context Inference:** Automatically parses local `.git/config` to determine the current `owner/repo`.
*   **Storage:** Implements the SDK's `TokenStorage` interface using the local file system (e.g., `~/.goddard/config.json`).
*   **User Experience:** Provides terminal spinners, colored output, formatting for real-time event streams, and automation loops via `pi-loop run`.

---

## Data & Execution Flow Example (Creating a PR)
1.  **Dev invokes CLI:** `goddard pr create -m "Fix memory leak"`
2.  **SDK executes:** The CLI formats the request and passes it to `@goddard/sdk`.
3.  **Backend validates:** The SDK calls the Cloudflare Worker API. The Worker checks the Turso DB to validate the session and fetch the user's GitHub identity.
4.  **App acts:** The Worker uses the `goddard[bot]` installation token to create the PR via the GitHub API, appending `Authored via CLI by @username` to the PR body.
5.  **Review happens:** A coworker comments on the PR on GitHub.com.
6.  **Webhook fires:** GitHub POSTs the comment event to the Worker.
7.  **Bot reacts:** The Worker immediately uses Octokit to add an 👀 reaction to the comment.
8.  **Stream broadcasts:** The Worker passes the payload to the Durable Object, which pushes it down the WebSocket.
9.  **Terminal updates:** The SDK parses the WebSocket frame, emits a `comment` event, and the CLI prints the comment natively in the developer's terminal.
