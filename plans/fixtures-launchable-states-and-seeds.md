# Fixtures, Launchable States, And Seeds

Product ambiguity status: resolved.

## Intent

Centralize reusable domain-shaped test and development data without turning that data into an app-specific mock layer, a production runtime dependency, or a daemon-specific seeding system.

Goddard now has three overlapping data needs:

- tests need schema-shaped records with stable defaults
- launchable app states need realistic query results for critical UI scenarios
- seed scripts need durable records for persistence, migration, smoke, or integration workflows

The shared answer should be a dev/test-only `core/fixtures` workspace package that owns deterministic data builders, response helpers, and pure scenario objects. Runtime-specific systems should compose those fixtures where they execute.

## Naming

Use `core/fixtures`, not `core/mock`.

`fixtures` better describes deterministic domain data. `mock` implies fake behavior, spies, stubbed clients, request interception, or service replacement. Those may be useful later, but they are a different concern from building valid `DaemonSession`, inbox item, pull request, history turn, and worktree records.

If runtime fake behavior becomes necessary later, use a separate surface such as package-local `test-support` or a dedicated `test-doubles` package. Do not overload `fixtures` with mocked clients or fake transports.

## Responsibilities

`core/fixtures` should own:

- schema-shaped object factories
- SDK response-envelope helpers when they remove repeated wrapping code
- stable ids and timestamps for repeatable scenarios
- domain scenario builders and a curated scenario catalog
- shared fixture constants that are platform-agnostic
- TypeScript types that make invalid fixture composition hard

`core/fixtures` should not own:

- app navigation
- app query-cache injection
- `state-launcher` wiring
- daemon database writes
- kindstore setup
- SDK client mocking
- test framework setup
- process, filesystem, network, or desktop-host behavior

The package should be boring, pure TypeScript. Importing it should not start a daemon, open a database, register commands, install globals, or depend on browser APIs.

Production app, daemon, SDK, and backend runtime entrypoints should not depend on `core/fixtures`. Tests, seed scripts, smoke harnesses, app development tooling, and launchable states may import it directly.

Fixture defaults must be synthetic. Do not encode real credentials, tokens, private repository data, or personal filesystem paths in shared fixtures. Use neutral paths and identities such as `/workspace/goddard-ai`, `fixture-user`, and `example` hostnames unless a consumer overrides them locally.

## Package Shape

Create a private workspace package under `core/fixtures`.

Example:

```txt
core/
  fixtures/
    package.json
    tsconfig.json
    src/
      index.ts
      ids.ts
      time.ts
      session.ts
      inbox.ts
      pull-request.ts
      scenarios.ts
      responses.ts
```

Package name:

```json
{
  "name": "@goddard-ai/fixtures",
  "private": true
}
```

The root workspace should include `core/fixtures` through the existing `core/*` workspace pattern.

## Factory Style

Factories should produce valid shared schema types with stable defaults and small override inputs.

Use both stable named ids and deterministic generated ids:

- exported scenarios use stable, human-readable ids so failures, snapshots, and seeded records are easy to inspect
- low-level factories generate deterministic ids by default so local callers do not hand-maintain ids for every object
- callers may override any generated id when a cross-record reference or snapshot contract needs a specific value

Example shape:

```ts
export function createDaemonSession(input: Partial<DaemonSession> = {}) {
  const id = input.id ?? nextFixtureSessionId()

  return {
    id,
    acpSessionId: input.acpSessionId ?? `${id}_acp`,
    status: "idle",
    agentName: "Codex",
    cwd: fixtureProjectPath,
    title: "Fixture session",
    createdAt: fixtureNow,
    updatedAt: fixtureNow,
    ...input,
  } satisfies DaemonSession
}
```

Scenario builders should pass explicit ids for records that are part of the named scenario's public shape. Factory-generated ids are fine for internal supporting records that no consumer references directly.

Factories should prefer explicit domain defaults over optional helper flags. If a scenario needs an active, blocked, or failed session, use a scenario builder or a narrowly named helper instead of growing one factory with many boolean options.

Good:

- `createDaemonSession()`
- `createBlockedSessionScenario()`
- `createPullRequestAttentionScenario()`

Avoid:

- `createSession({ withInbox: true, withPr: true, withWorktree: true, blocked: true })`

## Scenario Builders

Scenario builders should return plain data bundles and response envelopes, not runtime behavior.

Example:

```ts
export function createBlockedSessionScenario() {
  const session = createDaemonSession({
    id: "ses_scenario_blocked",
    status: "blocked",
  })

  return {
    session,
    inboxItem: createInboxItem({
      entityId: session.id,
      reason: "session.blocked",
      status: "unread",
    }),
    history: createSessionHistoryResponse({ session }),
    sessionResponse: createGetSessionResponse({ session }),
    worktree: createSessionWorktreeResponse({ session }),
    changes: createSessionChangesResponse({ session }),
  }
}
```

The scenario builder should not know whether its output will be injected into app queries, inserted into a database, serialized into a snapshot, or used in tests.

Response helpers may import the shared response types they satisfy, including SDK-exported response types when that is the clearest stable source. They must not import SDK clients, daemon clients, or runtime transports.

The scenario catalog can be broader than the initial launchable states. It should still stay curated: add scenarios that represent real product states, useful review surfaces, seed inputs, or repeated test conditions. Do not create a disconnected sample universe whose records have no consumer.

## Launchable States

Launchable states should stay app-local under `app/src/dev/`.

The app dev layer should:

- define and register `state-launcher` commands
- mount the launcher UI
- navigate app state such as selected main tab or opened workbench tabs
- inject query results into `queryClient`
- return cleanup from launch handlers

The app dev layer should not hand-author large domain records once `core/fixtures` exists. It should compose shared fixture scenarios and response helpers into app-specific query injection.

Example:

```ts
const scenario = createBlockedSessionScenario()

return composeCleanups([
  queryClient.injectData(
    goddardSdk.session.get,
    [{ id: scenario.session.id }],
    scenario.sessionResponse,
  ),
  queryClient.injectData(
    goddardSdk.inbox.list,
    [getInboxListRequest()],
    scenario.inboxListResponse,
  ),
])
```

Launchable states are the default tool for UI and product review scenarios because they are explicit, fast, reversible, and do not require maintaining a durable world of sample data.

## Seeds

Seeds should be composed from `core/fixtures`, but seed execution should not live in `core/fixtures`.

Seed runners belong where they execute:

- daemon database seeders near daemon or persistence code
- migration fixtures near migration tests
- smoke seed scripts near smoke harnesses
- app query scenario adapters under `app/src/dev/`

Seed runners own environment details:

- database path
- kindstore setup
- insert/update ordering
- process lifecycle
- cleanup strategy
- fixture-to-storage conversion

`core/fixtures` owns only the pure data inputs those seeders consume.

## Seed Policy

Launchable states should replace broad demo database seeds.

Keep seeded databases only for behavior that launchable query states cannot exercise well:

- persistence contracts
- migrations
- reload-after-restart behavior
- daemon and SDK integration
- cross-process smoke checks
- corruption or migration regression cases

Avoid a large always-on seeded demo universe. Broad durable seeds rot quickly and make ownership unclear. Prefer named seed scenarios that map to a specific contract or smoke workflow, even if the underlying fixture catalog contains more UI and test scenarios.

## Testing Policy

Package tests should use `core/fixtures` when they need shared domain records. They should still keep tiny inline objects when the data is local, obvious, and unlikely to be reused.

Use shared fixtures when:

- the object has many required schema fields
- multiple packages need the same shape
- the scenario represents a real product state
- manual duplication is already causing drift

Keep local test data when:

- the object is tiny
- the test is exercising a narrow parser or transform
- shared fixture defaults would obscure the assertion

`core/fixtures` should have its own tests for factory validity and scenario invariants, but consumers should test their own behavior through their public interfaces.

## Scenario Catalog

Start with the scenarios already useful for launchable app states:

- inbox attention queue with unread session blockers and pull request updates
- session triage queue with active, blocked, failed, and completed sessions
- blocked session detail with permission request, history, worktree, and diff data

Then expand into a broader curated catalog as repeated needs appear. Good candidates include:

- empty, sparse, and overloaded inbox queues
- sessions with active work, blocked permission requests, launch errors, completed history, and archived records
- pull request records with created, updated, replied, and completed attention states
- session history with plain text, tool calls, permission requests, plan updates, context usage, and errors
- worktree and change snapshots for clean, dirty, missing, and non-git workspaces

These give the package immediate reuse across app dev states, session UI tests, inbox UI tests, smoke setup, and future seed scripts.

## Migration Sequence

1. Create `core/fixtures` with session, inbox, pull-request, response, and scenario builders.
2. Move the large domain records from `app/src/dev/query-results.ts` into fixture builders and response helpers.
3. Keep app-specific query keys, navigation, and query injection in `app/src/dev/`.
4. Replace duplicated app tests that construct full `DaemonSession` or inbox records by hand when doing so improves clarity.
5. Grow the scenario catalog around repeated test, review, smoke, and seed needs.
6. Add daemon seed scripts only for persistence, migration, or smoke needs that launchable states do not cover.
7. Retire broad demo seed data if equivalent review states exist as launchable states.
8. Add package README guidance once at least two consumers use `core/fixtures`.

Early test migrations should target files with large hand-built `DaemonSession`, inbox item, pull request, or session history records. Do not migrate tiny inline objects whose local shape makes the assertion easier to understand.

## Non-Goals

Do not add:

- a mock SDK
- a fake daemon
- a fake backend
- transport interception
- app-only query injection helpers
- state-launcher APIs
- database seed runners
- generated fixture catalogs
- a public fixture API commitment

The first goal is to remove duplicated domain object construction while keeping runtime behavior owned by the package or app layer that actually runs it.
