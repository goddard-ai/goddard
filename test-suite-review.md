# Test Suite Review

Date: 2026-03-21

This review covers the current checked-in test suite across the workspace, including `app/`.

## Review Method

- This was a static review of the repository contents.
- I could not execute `pnpm test` in this checkout because dependencies are not installed and `turbo` is unavailable.
- Any statements about counts are based on checked-in `*.test.ts` files and static `test()` / `it()` declarations, not a live run.

## Current Snapshot

- The repo currently contains 39 test files and 168 statically declared test cases.
- `pnpm test` delegates to `turbo run test`, so only workspace packages with a `test` script participate in the normal package test graph.
- Packages with no test files today:
  - `core/config`
  - `core/tree-kill`
- Packages that still use `vitest run --passWithNoTests` despite having test files:
  - `core/daemon`
  - `core/daemon/client`
  - `core/paths`
  - `core/worktree`

## Top Findings

### 1. Assertion style drift is widespread

- The repo policy says to prefer `expect` in Vitest files.
- Current test files still import or use `assert` in multiple packages:
  - `workforce/test/main.test.ts`
  - `app/src/daemon-session.test.ts`
  - `core/backend/test/backend.test.ts`
  - `core/backend/test/github-app.test.ts`
  - `core/backend/test/router.test.ts`
  - `core/backend/test/worker.test.ts`
  - `core/daemon/client/test/index.test.ts`
  - `core/schema/test/routes.test.ts`
  - `core/schema/test/daemon-routes.test.ts`
  - `core/sdk/test/actions.integration.test.ts`
  - `core/sdk/test/actions.test.ts`
  - `core/sdk/test/daemon/ipc-client.test.ts`
  - `core/sdk/test/daemon/session-client.e2e.test.ts`
  - `core/sdk/test/workforce.test.ts`

Recommendation:

- Convert these incrementally as the files are touched.
- The schema route tests are the clearest low-effort cleanup because they are already minimal.

### 2. Shared storage mocks still drift across packages

- Storage-backed session behavior now has real tests in `core/paths/test/`, which is good.
- Even so, large inline mock implementations still exist in:
  - `app/src/daemon-session.test.ts`
  - `core/daemon/test/session-lifecycle.test.ts`
  - `core/sdk/test/actions.integration.test.ts`
  - `core/sdk/test/daemon/session-client.e2e.test.ts`
- These mocks duplicate storage semantics that the real storage package now defines directly.

Why this matters:

- Session timestamps, null defaults, history behavior, and permission updates can drift quietly.
- The suite can stay green while real storage behavior changes underneath it.

Recommendation:

- Extract shared fixtures for session storage and session permissions.
- Prefer thin fixtures that mirror the `core/paths` contract rather than hand-maintained per-file maps.

### 3. The backend production persistence path is still effectively untested

- `core/backend/test/` covers the in-memory control plane and router/server behavior well.
- I found no tests targeting `TursoBackendControlPlane`, `createSseSession()`, or `formatSseDataFrame()`.
- The real backend worker path still depends on `core/backend/src/db/persistence.ts` and `core/backend/src/utils.ts`.

Recommendation:

- Add one lean contract suite for `TursoBackendControlPlane`.
- Cover persisted session expiry, PR creation, managed PR checks, and reply authorization.
- Add one focused SSE utility test rather than growing more end-to-end worker coverage first.

### 4. Some important daemon and SDK utility contracts remain untested

- I found no direct tests for:
  - `core/daemon/src/ipc/socket.ts`
  - `core/daemon/src/config.ts`
  - `core/daemon/src/feedback.ts`
  - `core/daemon/src/utils.ts`
  - `core/daemon/src/loop/rate-limiter.ts`
- In `core/sdk`, the checked-in HTTP-side tests still miss the most important happy paths:
  - stream success-path parsing
  - stream close / abort behavior
  - `auth.login()` polling behavior
  - `pr.reply()` happy path

Recommendation:

- Add compact contract tests for these small modules instead of expanding already-large integration suites.

## What Improved Since The Last Review

- `core/paths` is no longer a zero-test package.
  - It now has real tests for token storage, session state storage, session permissions storage, and database-backed session / loop storage.
- `core/tree-kill` now has a `test` script, even though it still has no test files.
- `workforce/` now has a dedicated CLI test suite.

## What Is Working Well

### Strong behavioral coverage

- `core/daemon/test/session-lifecycle.test.ts` is still the strongest suite in the repo.
  - It covers reconnects, pagination, worktree behavior, diagnostics, abnormal exits, and archived history behavior.
- `core/daemon/test/workforce.test.ts` and `core/daemon/test/loop.test.ts` cover the daemon-owned background runtimes at a useful behavior level.
- `core/sdk/test/daemon/session-client.e2e.test.ts` still provides high-value daemon integration coverage from the SDK side.
- `core/backend/test/backend.test.ts` exercises user-visible login, PR creation, managed PR checks, and unified stream behavior through the HTTP server.
- `core/paths/test/db.test.ts` gives the suite a real persistence contract for session and loop records.
- `core/ipc/test/ipc.test.ts` stays lean and useful by checking request validation, structured errors, and NDJSON streaming over a real socket.

### Good alignment with the repository testing policy

- The best suites check package boundaries and user-visible behavior instead of private helpers.
- Recent additions in `core/paths` are compact and contract-oriented.

## Packages Needing More Attention

### `workforce`

- Status: useful CLI coverage, but it leans heavily on mocks and still uses `assert`.
- Recommendation: keep it focused on command routing and argument behavior; avoid adding deeper implementation assertions.

### `core/backend`

- Status: strong in-memory HTTP coverage, weak production persistence coverage.
- Recommendation: add a persistence-backed contract suite and SSE utility tests.

### `core/backend/client`

- Status: lean and sensible.
- Recommendation: low priority unless backend route contracts change.

### `core/config`

- Status: no tests.
- Recommendation: still acceptable unless the package starts validating or transforming config.

### `core/daemon`

- Status: strongest package in the suite, but still missing utility-contract coverage.
- Recommendation: add small direct tests for socket, config, feedback, repo parsing, and loop rate limiting.

### `core/daemon/client`

- Status: narrow but useful.
- Recommendation: add negative env cases and precedence checks.

### `core/ipc`

- Status: good lean contract suite.
- Recommendation: add malformed-stream and unsubscribe cleanup cases when convenient.

### `core/schema`

- Status: still shallow.
- Recommendation: keep only the stable-path assertions that matter and add a few real schema parsing checks.

### `core/sdk`

- Status: strong around daemon integration, thinner around backend HTTP success paths.
- Recommendation: add stream success-path, close behavior, `auth.login()`, and `pr.reply()` coverage.

### `core/paths`

- Status: meaningfully improved and now worth treating as a contract source for shared fixtures.
- Remaining gaps:
  - path resolution helpers such as loop config precedence
  - malformed file edge cases beyond token storage

### `core/tree-kill`

- Status: no tests.
- Recommendation: add a tiny suite only if this package becomes more than a thin utility wrapper.

### `core/worktree`

- Status: adequately covered, but still the most implementation-shaped package in the suite.
- Why:
  - the tests mock `spawnSync` and `existsSync` heavily
  - several assertions track fallback strategies and command behavior closely
- Recommendation:
  - keep plugin-selection and fallback outcomes
  - avoid growing command-level assertions further

## Priority Next Steps

1. Add a persistence-backed contract suite for `TursoBackendControlPlane`.
2. Add direct tests for daemon socket/config/feedback/utils and loop rate limiting.
3. Add missing SDK HTTP happy-path coverage for stream success, close semantics, `auth.login()`, and `pr.reply()`.
4. Extract shared storage fixtures so daemon, SDK, and app-adjacent tests stop hand-rolling storage semantics.
5. Convert the remaining `assert`-based Vitest files to `expect` as opportunistic cleanup.

## Bottom Line

The suite is materially better than the previous review described. `core/paths` is no longer a blind spot, and the daemon-centered behavioral coverage remains the strongest part of the repo.

The highest-value issues now are different:

- assertion style has drifted away from repo policy
- backend production persistence still lacks contract coverage
- several small daemon and SDK utility contracts remain untested

The next round should stay lean and focus on those gaps rather than broadly adding more tests everywhere.
