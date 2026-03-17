# Test Suite Review

Date: 2026-03-17

This review covers every package in the workspace except `app/`, per request.

## Executive Summary

The current non-app suite is directionally good: it is mostly behavioral, it exercises real cross-package flows in the daemon and SDK, and it already protects several important boundaries such as daemon token enforcement, restart reconciliation, and backend HTTP wiring.

The main problems are not broad test quantity problems. They are:

- Large blind spots in core persistence and platform-adapter packages.
- A production-path gap in `core/backend` where the in-memory control plane is tested but the Turso-backed control plane is not.
- Several duplicated inline mocks across `daemon/` and `core/sdk/` that are already drifting away from the real storage package goal.
- A small cluster of tests that are too implementation-shaped, especially in `core/worktree`.

`pnpm test` is currently green for the non-app workspace. The run covered 82 tests across these packages:

- `daemon`: 25
- `core/sdk`: 21
- `core/backend`: 13
- `core/worktree`: 10
- `core/schema`: 5
- `core/ipc`: 4
- `daemon/client`: 4

Packages currently passing with no tests:

- `core/storage`
- `core/config`
- `core/tauri-plugin-ipc`

Additional gap:

- `core/tree-build` has no `test` script, so the recursive workspace test run does not exercise it at all.

## What Is Working Well

### Strong, high-value coverage

- `daemon/` is the strongest part of the suite. It covers session lifecycle behavior, security boundaries, restart recovery, one-shot flows, CLI tool integration, and some real storage behavior.
- `core/sdk/` has useful end-to-end coverage against a real daemon server instead of only mocking everything at the boundary.
- `core/backend/` validates important user-facing flows through actual HTTP requests, including device auth, PR creation, managed-PR ownership checks, malformed JSON handling, and SSE delivery.
- `core/ipc/` tests the transport at the contract level rather than inspecting private helpers.

### Tests that match the stated philosophy

- `daemon/test/ipc-security.test.ts`
- `daemon/test/session-lifecycle.test.ts`
- `daemon/test/restart-reconciliation.integration.test.ts`
- `core/sdk/test/daemon/session-client.e2e.test.ts`
- `core/sdk/test/actions.integration.test.ts`
- `core/backend/test/backend.test.ts`
- `core/backend/test/client.test.ts`
- `core/ipc/test/ipc.test.ts`

These are mostly checking behavior that matters to users or cross-package compatibility, not internal helper structure.

## Critical Missing Tests

These are the additions I would prioritize first while keeping the suite lean.

### 1. `core/storage` has zero tests despite being foundational

This is the biggest gap in the workspace.

Why it matters:

- The daemon depends on `SessionStorage`, `SessionStateStorage`, and `SessionPermissionsStorage` for durability and restart recovery.
- The SDK and backend depend on token and path behavior from this package.
- Current daemon and SDK tests often replace storage with local hand-rolled mocks instead of checking the real storage contract.

Critical missing tests:

- `FileTokenStorage` round-trip, overwrite, clear, and malformed JSON recovery.
- `resolveLoopConfigPath()` local-over-global precedence and missing-file behavior.
- `SessionStateStorage` create/get/list/update/appendHistory/appendDiagnostic/remove using real filesystem state.
- `SessionPermissionsStorage` create/get/getByToken/addAllowedPr/revoke, including idempotent add behavior.
- `SessionStorage` and `LoopStorage` minimal CRUD tests against a temporary real DB.

Recommendation:

- Add a small real-storage contract suite in `core/storage/test/`.
- Then have daemon and SDK tests reuse helpers built on the real storage contract where practical, instead of repeating local in-memory maps.

### 2. `core/backend` does not test the production persistence path

Current backend tests exercise `InMemoryBackendControlPlane` well, but not `TursoBackendControlPlane`.

Why it matters:

- `core/backend/src/db/persistence.ts` is the real production implementation.
- It contains behavior that is not identical to the in-memory control plane.
- A regression there would not be caught by the current suite.

Critical missing tests:

- `completeDeviceFlow()` persists user and auth session correctly.
- `getSession()` rejects expired rows.
- `createPr()` writes row data correctly and assigns final PR number and URL.
- `isManagedPr()` enforces owner/repo/prNumber/createdBy matching.
- `replyToPr()` rejects unmanaged PRs before trying to post via app.

Recommendation:

- Add a compact contract suite that runs the same behavior checks against both control-plane implementations.
- Keep the in-memory tests for speed, but add one persistence-backed suite so the production path is not untested.

### 3. `core/sdk` is missing success-path stream and auth-loop coverage

Current SDK coverage is good around daemon sessions, but thin around the plain HTTP SDK behavior.

Critical missing tests:

- `GoddardSdk.stream.subscribeToRepo()` successful SSE parsing across chunk boundaries.
- Subscription close semantics: close emits once, aborts the fetch, and stops further events.
- `auth.login()` polling behavior for `authorization_pending`, `slow_down`, and timeout.
- `pr.reply()` authenticated happy path and unauthenticated rejection.

Why this matters:

- These are user-visible SDK behaviors, not implementation details.
- The malformed-stream test is useful, but without the corresponding success-path test the stream parser is under-covered.

### 4. `daemon/src/ipc/socket.ts` needs direct safety tests

The daemon suite exercises a lot of IPC behavior, but not the socket-path safety logic directly.

Critical missing tests:

- `prepareSocketPath()` removes stale sockets on `ENOENT` and `ECONNREFUSED`.
- `prepareSocketPath()` refuses to clobber an actively listening daemon.
- `cleanupSocketPath()` is safe and idempotent.

Why this matters:

- This is a correctness and safety boundary for daemon startup.
- A bug here can break multi-daemon behavior or destroy a live socket.

### 5. `core/tauri-plugin-ipc` has zero tests even though it is a core capability bridge

This is not the `app/` package, so it should not be left unreviewed just because it is Tauri-facing.

Critical missing tests:

- `createTauriTransport().send()` forwards the right invoke payload.
- `subscribe()` filters by subscription id, socket path, and stream name.
- Failed subscribe cleanup calls `unlisten()` and does not leak listeners.
- Unsubscribe calls `plugin:ipc|unsubscribe` exactly once.

Why this matters:

- This is the core-side implementation of app IPC capability.
- The repo instructions require app/core parity when adding capabilities. A contract test here helps enforce that without adding `app/` tests.

## Packages With No Tests

### `core/storage`

Status: critically under-tested.

Recommendation: add tests now.

### `core/tauri-plugin-ipc`

Status: materially under-tested.

Recommendation: add a lean mocked transport contract suite now.

### `core/config`

Status: no tests, but this is not a major risk today.

Why this is lower priority:

- `configSchema` is intentionally permissive and mostly passthrough.
- Testing constant exports and passthrough identity would add little value right now.

Recommendation:

- Do not add tests just to test constants.
- Revisit only if this package starts validating or transforming config.

### `core/tree-build`

Status: completely outside the current test loop because it has no `test` script.

Recommendation:

- Either add a tiny test target, or explicitly document that this is an intentionally untested dev script.
- If retained, the most valuable lean tests would cover dependency-graph ordering, cycle detection, and the recursion guard via `MONOREPO_BUILD_IN_PROGRESS`.

## Tests That Are Unnecessary or Too Implementation-Shaped

These are not severe problems, but they are the places I would trim or rewrite if the suite needs to stay lean.

### `core/worktree/test/worktree.test.ts`

This file is the clearest case of over-specifying implementation details.

Why:

- Many assertions are about exact shell commands and exact fallback sequencing.
- The tests heavily mock `spawnSync` and `fs.existsSync`, which makes them sensitive to refactors that preserve behavior.

What to keep:

- Plugin selection behavior.
- Fallback from worktrunk to default plugin.
- Cleanup behavior at the plugin-contract level.

What to trim or rewrite:

- Exact `cp` flag selection.
- Exact shell invocation ordering when multiple fallback mechanisms are possible.
- Assertions that mostly restate current implementation rather than user-visible outcome.

Preferred replacement:

- Smaller plugin-contract tests around `setup()` and `cleanup()` outcomes.
- One or two focused integration-ish tests with a temporary git repo if that remains practical.

### `core/schema/test/routes.test.ts` and `core/schema/test/daemon-routes.test.ts`

These tests are not wrong, but they are too shallow to carry much value on their own.

Why:

- They mostly assert string literals for path sources.
- They do not cover the request/response/query/header schemas that actually define the contract.

Recommendation:

- Keep only the route-path assertions that represent truly stable external contracts.
- Add a few contract tests that parse representative route inputs instead of growing the path-only tests.

### `core/schema/test/daemon-types.test.ts`

This is partly useful as a guard against exporting runtime schemas by accident, but the shape check is incomplete.

Why:

- The second test only proves that `id` and `acpId` are keys, not that the rest of the contract is correct.
- It reads more like a narrow regression check than a full contract test.

Recommendation:

- Keep the “no runtime schemas leaked” assertion if that is intentional.
- Replace or expand the key-shape check with a clearer contract-level assertion if the goal is long-term schema drift protection.

## Incomplete Coverage Inside Already-Tested Packages

### `core/ipc`

Current suite is good but incomplete.

Missing worthwhile tests:

- Invalid JSON or malformed NDJSON on the stream path.
- Unsubscribe behavior and closed-client cleanup.
- Error propagation for non-JSON error bodies from the server transport.

These would still be contract tests, not implementation tests.

### `daemon/client`

Current coverage focuses on happy-path env resolution.

Missing worthwhile tests:

- Missing `GODDARD_SESSION_TOKEN` throws a useful error.
- `GODDARD_DAEMON_URL` vs `GODDARD_DAEMON_SOCKET_PATH` precedence.
- Default socket path resolution.

This is cheap to add and keeps the daemon-client contract explicit.

### `daemon`

The main suite is strong, but a few thin modules are currently untested:

- `daemon/src/config.ts`
- `daemon/src/feedback.ts`
- `daemon/src/utils.ts`

Recommended additions:

- `resolveDaemonRuntimeConfig()` precedence rules.
- `prependAgentBinToPath()` behavior with and without existing `PATH`.
- `buildPrompt()` for comment vs review events.
- `splitRepo()` valid and invalid input.

These are small but high-signal tests because the logic is simple and public.

### `core/backend`

Thin entrypoints like `src/server.ts` and `src/worker.ts` do not need dedicated tests yet.

What is missing instead:

- Contract tests for `createSseSession()` and `formatSseDataFrame()`.
- More router-level auth and error-path tests for `prReplyRoute`.

### `core/sdk`

In addition to the critical items above, these are worth adding eventually:

- Negative cases for malformed action frontmatter/config.
- Missing `prompt.md` in folder actions.
- `core/sdk/src/node/agents.ts` idempotent `AGENTS.md` mutation behavior.
- Direct `RateLimiter` tests for cron expressions and invalid delay fallback.

## Shared Mocks and Drift

The current suite does not yet match the stated preference for shared mocks checked for drift.

### Current duplication

The same storage mock shape appears repeatedly in:

- `daemon/test/session-lifecycle.test.ts`
- `core/sdk/test/actions.integration.test.ts`
- `core/sdk/test/daemon/session-client.e2e.test.ts`

Very similar daemon test-server helpers also appear in:

- `daemon/test/ipc-security.test.ts`
- `core/sdk/test/daemon/ipc-client.test.ts`

### Why this is risky

- If `@goddard-ai/storage` changes, these local mock shapes can remain green while the real package changes underneath them.
- The more these mocks are copied, the more likely they diverge subtly in timestamps, null defaults, or persistence behavior.

### Recommendation

Move toward this pattern:

1. Create shared test fixtures in a common helper location for daemon/session storage and daemon IPC server setup.
2. Add a small real-storage contract suite in `core/storage/test/` that defines the behavior those fixtures must emulate.
3. Keep using mocks where they buy speed, but derive them from one shared helper instead of repeating hand-built `Map` implementations.

That keeps the suite lean while making drift visible.

## Package-by-Package Assessment

### `daemon`

- Overall: strong.
- Keep: lifecycle, security, restart, one-shot, CLI integration coverage.
- Add: socket/config/prompt helper tests.
- Risk level: low-to-medium after storage and socket-path gaps are addressed.

### `daemon/client`

- Overall: decent but narrow.
- Add: negative env cases and precedence tests.
- Risk level: medium because this package hides important env conventions.

### `core/sdk`

- Overall: strong around daemon integration, incomplete around HTTP SDK behavior.
- Add: stream success path, `auth.login`, `pr.reply`, action failure modes, `RateLimiter`.
- Risk level: medium.

### `core/backend`

- Overall: good in-memory behavior coverage, weak production-path coverage.
- Add: Turso control-plane contract tests.
- Risk level: high until the persistence implementation is exercised.

### `core/ipc`

- Overall: good lean contract coverage.
- Add: stream cleanup and malformed-stream edge cases.
- Risk level: low-to-medium.

### `core/schema`

- Overall: currently shallow.
- Keep: a minimal set of path/export guards where externally meaningful.
- Add: schema contract checks instead of only path-string checks.
- Risk level: medium because this package is the contract hub.

### `core/storage`

- Overall: biggest gap in the repo.
- Add: real persistence tests now.
- Risk level: high.

### `core/tauri-plugin-ipc`

- Overall: meaningful capability bridge with zero tests.
- Add: mocked transport contract tests now.
- Risk level: medium-to-high.

### `core/config`

- Overall: acceptable to leave mostly untested for now.
- Add tests only if validation logic grows.
- Risk level: low.

### `core/worktree`

- Overall: covered, but some of that coverage is too implementation-specific.
- Rewrite some tests toward plugin-contract behavior.
- Risk level: medium.

### `core/tree-build`

- Overall: untested and not even part of the recursive test contract.
- Decide whether it is intentionally out of scope; if not, add one tiny suite and a `test` script.
- Risk level: medium.

## Recommended Next Steps

If I were tightening this suite in priority order while keeping it lean, I would do this:

1. Add real tests for `core/storage`.
2. Add contract tests for `TursoBackendControlPlane`.
3. Add `core/tauri-plugin-ipc` transport tests.
4. Add SDK stream success-path and `auth.login` tests.
5. Extract shared daemon/storage test fixtures and reduce duplicated inline mocks.
6. Trim `core/worktree` tests down to behavior-level assertions.

## Bottom Line

The suite is already strongest where the product is most complex: daemon/session flows. That is good.

The next step is not to broadly add more tests everywhere. It is to close the specific blind spots in persistence, production backend behavior, and capability adapters, then simplify the handful of tests that currently lock in command-level implementation details.
