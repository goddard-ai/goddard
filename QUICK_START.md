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

### Step 3: Use the CLI (Terminal B)
```bash
# Login
pnpm --dir=cmd goddard login --username <your-github-username>

# Create a Pull Request
pnpm --dir=cmd goddard pr create --repo <owner/repo> --title "My local test PR"

# Start the PR-feedback daemon
pnpm --dir=daemon daemon run --repo <owner/repo> --project-dir $(pwd)

# Initialize and run autonomous loop
pnpm --dir=cmd goddard loop init
pnpm --dir=cmd goddard loop run
pnpm --dir=cmd goddard loop generate-systemd
```

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
4.  **CLI Distribution**:
    - The CLI is distributed via npm as `@goddard-ai/cmd`.

For architectural details, see [build.md](./build.md).
For standalone CLI documentation, see [cmd/QUICK_START.md](./cmd/QUICK_START.md).
