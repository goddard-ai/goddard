# TODO

## Remaining Human Requirements (Manual Setup)

### 2. Infrastructure & Production Secrets
- [ ] Provision a **Turso** database for the production control plane.
- [ ] Configure production secrets for the Cloudflare Worker:
  - `TURSO_DB_URL`
  - `TURSO_DB_AUTH_TOKEN`
- [ ] Provision a **GitHub App** for the production environment.
- [ ] Configure GitHub App credentials:
  - `GITHUB_APP_ID`
  - `GITHUB_PRIVATE_KEY`

## Technical Debt & Feature Gaps

### SDK & Architecture
- [ ] Implement `GoddardSdk.loop.spec` and `propose` in `@goddard-ai/sdk` (remove direct `pi` spawns in CLI).
- [ ] Centralize `FileTokenStorage` into `@goddard-ai/sdk/node`.
- [ ] Implement backend logic for `sdk.pr.isManaged` to filter events correctly.

### Daemon Improvements
- [ ] Implement automatic cleanup of `.goddard-agents/` worktrees after sessions finish.
- [ ] Improve PTY handling for non-tmux environments in one-shot sessions.

### Testing & Stability
- [ ] Resolve Vitest path mapping for workspace packages (ensure `pnpm test` works globally).
- [ ] Add end-to-end integration tests for the `goddard login` and `whoami` flow.
- [ ] Implement a local "Mock" mode for the backend to allow development without Turso.

## Why these remain

- This local agent environment cannot create GitHub secrets or provision external repositories.
- Production infrastructure (Turso/Cloudflare) and GitHub App configuration require human credentials and dashboard access.
- External repository URLs were not provided, so `.gitrepo` metadata could not be finalized.
