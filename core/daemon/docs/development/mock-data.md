# Mock Data

> The mock data profile gives app and SDK developers deterministic local daemon data that behaves like normal daemon data to clients. This page explains what gets seeded, what stays isolated, and what mock data is not meant to prove.

- **Core idea**
  - The mock data profile provides deterministic local-only daemon data for app and SDK development.
  - It is isolated from production and development daemon data.
  - It lets clients exercise realistic user-visible states through the normal daemon surface.

- **Seeding**
  - The daemon can seed the `mock` profile.
  - Seeding can optionally reset existing mock database artifacts before writing the fixture set.
  - Resetting mock data affects only the mock profile's database files.
  - Seeding does not contact external services.
  - Seeding does not create reconnectable live sessions.

- **Fixture intent**
  - The mock dataset is scenario-comprehensive rather than schema-comprehensive.
  - It covers user-visible screen states and workflows needed by app and SDK development.
  - Current fixture areas include:
    - daemon session scenarios across completed, blocked, errored, cancelled, archived, and idle-looking history states
    - session detail scenarios for multi-turn history, interrupted-style history, long copy, near-limit context usage, model state, config options, and slash command suggestions
    - inbox rows across unread, read, saved, archived, replied, and completed statuses
    - session-owned and pull-request-owned attention rows
    - local-only daemon-managed pull request records with created and updated attention

- **Boundaries**
  - Mock data is not production seed data.
  - Mock data is not a schema exhaustiveness test.
  - Future fixtures should be added only when they exercise a named user-visible screen state or workflow.
  - App and SDK callers should consume mock data through the normal daemon control surface rather than special test-only paths.
  - Related pages: [data profiles](../concepts/data-profiles.md), [inbox statuses](../attention/inbox-statuses.md), and [history and diagnostics](../sessions/history-and-diagnostics.md).
