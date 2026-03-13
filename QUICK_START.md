# QUICK START

This guide explains how to set up Goddard in two ways: **Local Development** and **Production Deployment**.

---

## 1. Local Development

Use this setup to test the full end-to-end integration (CLI -> SDK -> Backend) on your local machine.

### Prerequisites
- Node.js 22+
- `pnpm`

### Step 1: Install Dependencies
```bash
pnpm install
pnpm run check
```

### Step 2: Start the Backend (Terminal A)
```bash
pnpm --dir=backend start
# Backend starts on http://127.0.0.1:8787
```

### Step 3: Use the Daemon (Terminal B)
```bash
# Start the PR-feedback daemon
pnpm --dir=daemon daemon run --repo <owner/repo> --project-dir $(pwd)
```

---

## Recommended Workflow: PR Stacking

While Goddard's agents can adapt to various version control strategies, we strongly recommend a **PR Stacking** workflow (often facilitated by tools like [Git Town](https://www.git-town.com/) or Graphite).

*   **Smaller, Atomic PRs:** Stacked PRs naturally encourage smaller units of work, making code review faster and less error-prone.
*   **Agent Compatibility:** Autonomous loops perform better when they have clear boundaries. Giving an agent a targeted objective on a branch stacked on top of an existing, unmerged feature keeps its "blast radius" contained and its context focused.
*   **Configuration:** If you use tools like `git-town`, you can add specific instructions to your project's `AGENTS.md` file (via `goddard agents init`) to teach the `pi-coding-agent` your exact stacking commands and conventions.

---

## 2. Production Deployment

Goddard is designed to be deployed as a globally distributed control plane.

### Infrastructure Requirements
- **Cloudflare Workers**: Host the API and Durable Objects for SSE fan-out.
- **Turso**: Edge SQLite database for persistence.
- **GitHub App**: Handles official PR creation and webhook ingestion.

### Deployment Steps

1.  **Database Provisioning**:
    - Set up a Turso database and configure `TURSO_DB_URL` and `TURSO_DB_AUTH_TOKEN`.
2.  **GitHub App Registration**:
    - Create a GitHub App and configure `GITHUB_APP_ID` and `GITHUB_PRIVATE_KEY`.
3.  **Backend Deployment**:
    - Deploy the `backend/` package using `wrangler deploy`.
For architectural details, see [build.md](./build.md).
