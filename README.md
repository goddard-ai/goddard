# goddard

TypeScript-only monorepo for:

- `@goddard-ai/sdk`
- `@goddard-ai/backend`
- `@goddard-ai/github-app`
- `@goddard-ai/cmd`
- `@goddard-ai/daemon`

## Workspace

- `backend`
- `cmd`
- `daemon`
- `github-app`
- `sdk`

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
