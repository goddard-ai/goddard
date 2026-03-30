# TODO

## Remaining Human Requirements (Manual Setup)

### Infrastructure & Production Secrets
- [ ] Provision a **Turso** database for the production control plane.
- [ ] Configure production secrets for the Cloudflare Worker:
  - `TURSO_DB_URL`
  - `TURSO_DB_AUTH_TOKEN`
- [ ] Provision a **GitHub App** for the production environment.
- [ ] Configure GitHub App credentials:
  - `GITHUB_APP_ID`
  - `GITHUB_PRIVATE_KEY`

## Technical Debt & Feature Gaps

### Workforce
- [ ] Add daemon-enforced workforce git controls for handled sessions in the shared working tree.
- [ ] Preserve per-request git attribution, validation baselines, and audit data durably enough to survive daemon restart.
- [ ] Make `workforce respond` validate owned-path dirty state and attributed commits, suspending ownership violations for human review.
- [ ] Implement `workforce update` steering for active requests without requeueing the live session.
- [ ] Make workforce shutdown quiescent and expose richer runtime/request inspection plus lifecycle states beyond `running`.
- [ ] Tighten workforce config validation for duplicate agent ids, invalid root-agent topology, and overlapping ownership.

### SDK & Architecture
- [ ] Implement `GoddardSdk.loop.spec` and `propose` in `@goddard-ai/sdk` (remove direct `pi` spawns in CLI).

### Daemon Improvements
- [ ] Implement automatic cleanup of `.goddard-agents/` worktrees after sessions finish.
- [ ] Improve PTY handling for non-tmux environments in one-shot sessions.

### Testing & Stability
- [ ] Resolve Vitest path mapping for workspace packages (ensure `pnpm test` works globally).
- [ ] Add end-to-end integration tests for the `goddard login` and `whoami` flow.

## Status Notes

- Local backend development already has an in-memory control plane fallback when `DATABASE_URL` is unset.
- Production infrastructure (Turso/Cloudflare) and GitHub App configuration require human credentials and dashboard access.
- Workforce runtime scaffolding exists today; the main gaps are git enforcement, validation, lifecycle hardening, and operator visibility.
