# goddard

TypeScript-only monorepo for:

- `@goddard-ai/sdk`
- `@goddard-ai/backend`
- `@goddard-ai/github-app`
- `@goddard-ai/cmd`
- `@goddard-ai/daemon`

Core packages:

- `@goddard-ai/schema`
- `@goddard-ai/storage`

## Issues & Feature Requests

Please direct bug reports and feature requests to the Issues URL of the appropriate subpackage repository:

- **[goddard-ai/sdk](https://github.com/goddard-ai/sdk/issues)**: For issues related to the SDK.
- **[goddard-ai/backend](https://github.com/goddard-ai/backend/issues)**: For issues related to the backend service.
- **[goddard-ai/github-app](https://github.com/goddard-ai/github-app/issues)**: For issues related to the GitHub app.
- **[goddard-ai/cmd](https://github.com/goddard-ai/cmd/issues)**: For issues related to the CLI tools.
- **[goddard-ai/daemon](https://github.com/goddard-ai/daemon/issues)**: For issues related to the daemon.
- **[goddard-ai/core](https://github.com/goddard-ai/core/issues)**: For issues related to the shared core utilities (`schema` and `storage`).

## Workspace

- `backend`
- `cmd`
- `daemon`
- `github-app`
- `sdk`

Core packages:

- `core/schema`
- `core/storage`

## Quick start (local)

```bash
pnpm install
pnpm run check
```

### 1) Start the backend

```bash
pnpm --dir=backend start
```

Backend runs at `http://127.0.0.1:8787`.

### 2) Use the CLI

```bash
pnpm --dir=cmd goddard login --username <github-user>
pnpm --dir=cmd goddard whoami
pnpm --dir=cmd goddard pr create --repo owner/repo --title "Test PR" --head feature/demo --base main
pnpm --dir=cmd goddard spec
pnpm --dir=cmd goddard propose "summarize recent PR feedback"
pnpm --dir=cmd goddard loop init
pnpm --dir=cmd goddard loop run
pnpm --dir=cmd goddard loop generate-systemd
pnpm --dir=daemon daemon run --repo owner/repo --project-dir $(pwd)
```

### 3) Simulate a GitHub webhook

Use `@goddard-ai/github-app` to forward webhook events to backend:

```bash
pnpm --dir=github-app exec tsx --eval "import { createGitHubApp } from './src/index.ts'; (async () => { const app = createGitHubApp({ backendBaseUrl: 'http://127.0.0.1:8787' }); await app.handleWebhook({ type: 'issue_comment', owner: 'owner', repo: 'repo', prNumber: 1, author: 'teammate', body: 'Looks good' }); console.log('webhook sent'); })();"
```

## CI

- `.github/workflows/ci.yml` runs typecheck + tests on PRs and on `main` pushes.
- `.github/workflows/sync-subrepos.yml` pushes each subrepo via `git-subrepo` on `main`.

## Required secret for subrepo sync

- `SYNC_PAT`: token with write access to this repository and each external subrepo.
