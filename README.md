# Project Goddard

Goddard is a developer tooling suite and an experimental platform for building with autonomous AI coding agents.

This is a TypeScript-only monorepo containing:

- `@goddard-ai/sdk`
- `@goddard-ai/backend`
- `@goddard-ai/github-app`
- `@goddard-ai/cmd`
- `@goddard-ai/daemon`

Core packages:

- `@goddard-ai/schema`
- `@goddard-ai/storage`

---

## Hypothesis

We believe that by making software development an interactive partnership between a human developer and an autonomous AI coding agent, developers can dramatically improve their velocity and code quality. The AI is most effective when guided by structured, machine-readable domain models, proposals, and theories of mind (located inside a project's `spec/` directory), rather than open-ended unstructured chats. When we treat the AI as a peer that adheres to established Git and GitHub workflows, the transition to AI-assisted engineering becomes seamless.

## Value Proposition

Project Goddard provides a comprehensive toolkit for bringing an autonomous coding agent directly into your existing workflow. It bridges the gap between conversational AI interfaces and standard version control. Instead of copying and pasting code from a browser window, Goddard responds automatically to GitHub webhooks (like PR review comments) and drives the local development lifecycle—modifying code, running tests, and pushing commits—entirely autonomously.

## Capabilities

- **CLI (`@goddard-ai/cmd`)**: Powerful developer tools to manage your AI pair programmer (`goddard loop`), manage PRs, parse structured specifications, and translate human intent into actionable proposals.
- **Background Daemon (`@goddard-ai/daemon`)**: A local listener that spins up autonomous "pi sessions" to fix failing tests, implement feedback, and tackle features based on live events.
- **Backend & Webhooks (`@goddard-ai/backend`, `@goddard-ai/github-app`)**: Edge services that intercept GitHub events and instantly stream them to your local daemon via Server-Sent Events (SSE), enabling instant AI responses to PR feedback.
- **Extensible SDK (`@goddard-ai/sdk`)**: A core library containing the `pi-coding-agent`, logic for file parsing, AI model definitions, and system prompts.
- **PR Stacking**: First-class support for working with atomic, stacked pull requests to narrow the AI's "blast radius" and keep reviews manageable.

---

## Dual License Monorepo

To balance open-source flexibility with the sustainability of our platform, this monorepo utilizes a dual-licensing strategy:

- The root of the monorepo and specific services—such as the Backend service (`@goddard-ai/backend`) and the GitHub App package (`@goddard-ai/github-app`)—are licensed under the **GNU Affero General Public License v3.0 (AGPLv3)**.
- Reusable integration libraries and specific sub-packages—such as the SDK (`@goddard-ai/sdk`), the CLI (`@goddard-ai/cmd`), and the background daemon (`@goddard-ai/daemon`)—are licensed under the permissive **MIT License**. This allows for widespread adoption, easier integration into your own tools, and prevents virality for consumers of the SDK.

Please see the `LICENSE` file in the root directory and the respective `LICENSE` files in the subdirectories for exact legal details.

---

## Issues & Feature Requests

Please direct bug reports and feature requests to the Issues URL of the appropriate subpackage repository rather than this monorepo:

- **[goddard-ai/sdk](https://github.com/goddard-ai/sdk/issues)**: For issues related to the SDK.
- **[goddard-ai/backend](https://github.com/goddard-ai/backend/issues)**: For issues related to the backend service.
- **[goddard-ai/github-app](https://github.com/goddard-ai/github-app/issues)**: For issues related to the GitHub app.
- **[goddard-ai/cmd](https://github.com/goddard-ai/cmd/issues)**: For issues related to the CLI tools.
- **[goddard-ai/daemon](https://github.com/goddard-ai/daemon/issues)**: For issues related to the daemon.
- **[goddard-ai/core](https://github.com/goddard-ai/core/issues)**: For issues related to the shared core utilities (`schema` and `storage`).

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
