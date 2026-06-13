# `@goddard-ai/fixtures`

Synthetic, platform-agnostic data fixtures for development and tests.

This package owns reusable fixture builders, response-shaped helpers, stable IDs, stable timestamps, and curated scenario data. It exists to keep duplicated mock data out of app components, launchable-state wiring, tests, and seed adapters.

This package does not own app navigation, route state, query injection, `state-launcher` registration, database writes, kindstore setup, SDK client mocking, test framework setup, process behavior, filesystem behavior, network behavior, or desktop behavior.

Production app, daemon, SDK, and backend runtime entrypoints must not import this package. Seed code may compose fixture data in the future, but seed execution belongs to the package or app that performs the seeding.

## Package Boundary

Use this package from dev-only and test-only code when a scenario needs shared Goddard-shaped data:

- launchable app states can import scenarios and map them to app query keys in `app/src/dev`;
- tests can import low-level factories when repeated setup obscures the behavior being asserted;
- future seed adapters can import factories or scenarios and write them through their own persistence layer.

Do not import this package from production composition roots, daemon startup, SDK client construction, backend handlers, feature plugin runtime entrypoints, or process-level setup. A runtime module that needs real data should call the real SDK, daemon, backend, or persistence API instead.

## Data Policy

Fixture values are synthetic and deterministic. Stable exported scenarios use stable named IDs. Low-level factories generate deterministic IDs from their inputs. Defaults should be plausible enough to exercise UI and domain behavior, but they should not become a broad demo universe or imply that Goddard ships with seeded product data.

## Factories And Scenarios

Low-level factories build one schema-shaped record or response envelope:

```ts
import { createFixtureSession, createListSessionsResponse } from "@goddard-ai/fixtures"

const session = createFixtureSession({ title: "Review transcript state" })
const response = createListSessionsResponse([session])
```

Curated scenarios compose factories into named states that multiple consumers can share:

```ts
import { createSessionTriageQueueScenario } from "@goddard-ai/fixtures"

const scenario = createSessionTriageQueueScenario()

queryClient.injectData(goddardSdk.session.list, [{ limit }], scenario.response)
```

The scenario owns the data. The app still owns the query key, navigation target, and launch lifecycle.

## Seeds

Seeds may compose fixtures, but seed execution does not live here. A seed adapter should convert fixture records into writes for its own database or store:

```ts
import { createInboxAttentionQueueScenario, createSessionTriageQueueScenario } from "@goddard-ai/fixtures"

export async function seedDevDatabase(db: SeedDatabase) {
  const sessions = createSessionTriageQueueScenario()
  const inbox = createInboxAttentionQueueScenario({
    activeSession: sessions.activeSession,
    blockedSession: sessions.blockedSession,
  })

  for (const session of sessions.response.sessions) {
    await db.sessions.put(session.id, session)
  }

  for (const item of inbox.response.items) {
    await db.inbox.put(item.id, item)
  }
}
```

That adapter owns database connections, kindstore setup, migrations, transactions, cleanup, and environment selection. `@goddard-ai/fixtures` only supplies the data it writes.

## Current Seed-Like References

The current repository audit did not find a daemon or app database seed runner to move into this package. Existing seed-like references are mostly:

- worktree bootstrap file seeding in `features/session`;
- review-sync smoke/test fixtures in `core/review-sync`;
- an old daemon cleanup check for sessions marked with `metadata.mock === true`.

Those are process, filesystem, or test-harness concerns. They can consume fixtures later only if they need Goddard domain records, and their execution should stay in the owning package.
