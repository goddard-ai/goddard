# Fixtures, Launchable States, And Seeds

Product ambiguity status: proposed direction.

## Intent

Centralize reusable domain-shaped test and development data without turning that data into an app-specific mock layer or a daemon-specific seeding system.

Goddard now has three overlapping data needs:

- tests need schema-shaped records with stable defaults
- launchable app states need realistic query results for critical UI scenarios
- seed scripts need durable records for persistence, migration, smoke, or integration workflows

The shared answer should be a small `core/fixtures` workspace package that owns deterministic data builders and pure scenario objects. Runtime-specific systems should compose those fixtures where they execute.

## Naming

Use `core/fixtures`, not `core/mock`.

`fixtures` better describes deterministic domain data. `mock` implies fake behavior, spies, stubbed clients, request interception, or service replacement. Those may be useful later, but they are a different concern from building valid `DaemonSession`, inbox item, pull request, history turn, and worktree records.

If runtime fake behavior becomes necessary later, use a separate surface such as package-local `test-support` or a dedicated `test-doubles` package. Do not overload `fixtures` with mocked clients or fake transports.

## Responsibilities

`core/fixtures` should own:

- schema-shaped object factories
- stable ids and timestamps for repeatable scenarios
- small domain scenario builders
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

Example shape:

```ts
export function createDaemonSession(input: Partial<DaemonSession> = {}) {
  return {
    id: "ses_fixture_1",
    acpSessionId: "acp_fixture_1",
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

Factories should prefer explicit domain defaults over optional helper flags. If a scenario needs an active, blocked, or failed session, use a scenario builder or a narrowly named helper instead of growing one factory with many boolean options.

Good:

- `createDaemonSession()`
- `createBlockedSessionScenario()`
- `createPullRequestAttentionScenario()`

Avoid:

- `createSession({ withInbox: true, withPr: true, withWorktree: true, blocked: true })`

## Scenario Builders

Scenario builders should return plain data bundles, not runtime behavior.

Example:

```ts
export function createBlockedSessionScenario() {
  const session = createDaemonSession({
    id: "ses_fixture_blocked",
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
    worktree: createSessionWorktreeResponse({ session }),
    changes: createSessionChangesResponse({ session }),
  }
}
```

The scenario builder should not know whether its output will be injected into app queries, inserted into a database, serialized into a snapshot, or used in tests.

## Launchable States

Launchable states should stay app-local under `app/src/dev/`.

The app dev layer should:

- define and register `state-launcher` commands
- mount the launcher UI
- navigate app state such as selected main tab or opened workbench tabs
- inject query results into `queryClient`
- return cleanup from launch handlers

The app dev layer should not hand-author large domain records once `core/fixtures` exists. It should compose shared fixture scenarios into app-specific query results.

Example:

```ts
const scenario = createBlockedSessionScenario()

return composeCleanups([
  queryClient.injectData(goddardSdk.session.get, [{ id: scenario.session.id }], {
    session: scenario.session,
  }),
  queryClient.injectData(goddardSdk.inbox.list, [getInboxListRequest()], {
    items: [scenario.inboxItem],
    nextCursor: null,
    hasMore: false,
  }),
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

Avoid a large always-on demo universe. Broad seeds rot quickly and make ownership unclear. Prefer small named seed scenarios that map to a specific contract or smoke workflow.

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

## Initial Critical Scenarios

Start with the scenarios already useful for launchable app states:

- inbox attention queue with unread session blockers and pull request updates
- session triage queue with active, blocked, failed, and completed sessions
- blocked session detail with permission request, history, worktree, and diff data

These give the package immediate reuse across app dev states, session UI tests, inbox UI tests, and future seed scripts.

## Migration Sequence

1. Create `core/fixtures` with session, inbox, pull-request, and scenario builders.
2. Move the large domain records from `app/src/dev/query-results.ts` into fixture builders.
3. Keep app-specific SDK response wrapping in `app/src/dev/query-results.ts`.
4. Replace duplicated app tests that construct full `DaemonSession` or inbox records by hand when doing so improves clarity.
5. Add daemon seed scripts only for persistence, migration, or smoke needs that launchable states do not cover.
6. Retire broad demo seed data if equivalent review states exist as launchable states.
7. Add package README guidance once at least two consumers use `core/fixtures`.

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

## Open Questions

- Should fixture ids be human-readable stable strings, generated through helper counters, or both?
- Should scenario builders return SDK response envelopes, or should envelopes stay with each consumer?
- Should `core/fixtures` import only feature schemas, or is importing `@goddard-ai/sdk` types acceptable for response-shaped helpers?
- Which existing tests are noisy enough to migrate first without making assertions less direct?
