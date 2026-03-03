# Deployment & Build Status

## Current Build Status (Local MVP)

Implemented in this repository:
- SDK-first architecture (`sdk/`) used by CLI (`cmd/`) and integrations.
- Local backend control plane with auth flow endpoints, PR creation, webhook ingest, and WebSocket repo streams.
- GitHub App shim (`github-app/`) that forwards webhook events to backend.
- Monorepo CI and subrepo sync workflow scaffolding.

Latest hardening pass:
- Added auth/session expiration checks in backend.
- Added request body size limits and explicit invalid JSON handling.
- Added SDK stream payload guards so malformed frames emit `error` instead of crashing listeners.
- Added tests for session expiry, invalid JSON handling, and malformed stream payloads.

---

## Path to Production Deployment

While the local MVP is fully functional and tested, the following work is required to transition to the production architecture:

### A. Persistence & Infrastructure (The "Control Plane")
*   **Database Migration:** Replace the `InMemoryBackendControlPlane` with a production implementation using **Turso** (SQLite at the edge) and **Drizzle ORM**.
    *   Define schema for `users`, `auth_sessions`, and `pull_requests` in `backend/src/schema.ts`.
    *   Use **`drizzle-kit`** for all schema management. The Drizzle schema file is the single source of truth; `drizzle-kit generate` diffs the schema against the last snapshot and emits versioned migration SQL files into `backend/migrations/`.

    #### Schema management with `drizzle-kit`

    `backend/src/schema.ts` is the **single source of truth** for table structure. To generate a new migration after editing the schema:

    ```bash
    cd backend
    pnpm db:generate   # drizzle-kit generate
    ```

    To apply pending migrations to the database:

    ```bash
    pnpm db:migrate    # drizzle-kit migrate
    ```

    When you need to add a column or a new table, **edit `backend/src/schema.ts`** and run `pnpm db:generate`. Drizzle Kit snapshots the previous schema state, diffs it against the new one, and writes only the necessary `ALTER TABLE` statements into a new timestamped migration file. Never write migration SQL by hand.

    **What lives in `backend/src/schema.ts`:**
    - Table definitions (`sqliteTable`), columns, types, primary keys, `autoIncrement`, `notNull`, `references` (foreign keys).
    - Anything exported and consumed by Drizzle ORM at runtime.

    **What `drizzle-kit` manages automatically:**
    - Versioned migration files in `backend/migrations/` (SQL + snapshot JSON).
    - A `__drizzle_migrations` tracking table in the target database so `migrate` is idempotent.

*   **Cloudflare Workers Deployment:**
    *   Port the `backend` package to the Cloudflare Workers runtime.
    *   Replace the in-memory WebSocket management with **Cloudflare Durable Objects** for scalable, real-time broadcasting.
    *   Add `wrangler.toml` for deployment configuration.
*   **Production Secrets:**
    *   Provision and configure `TURSO_DB_URL` and `TURSO_DB_AUTH_TOKEN` in the production environment.
    *   Configure GitHub App credentials (`GITHUB_APP_ID`, `GITHUB_PRIVATE_KEY`) for real PR creation.

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

**Status note:** Local MVP runtime is deployable today (backend + CLI + SDK + webhook bridge) for transient testing. Production persistence and automated subrepo publishing require the infrastructure and metadata setup described above.
