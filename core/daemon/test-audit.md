# Core Daemon Test Audit

Use this guide to audit `core/daemon` tests before broad test refactors. The goal is to classify what each test protects, identify assertion surfaces that couple tests to implementation details, and propose stable seams for future maintenance.

Keep the audit separate from refactoring. First map the existing tests, then plan implementation work from the map.

## Discovery

List daemon test cases with semantic entities:

```bash
sem entities core/daemon/**/*.test.ts
```

For each test entity, inspect only enough surrounding code to classify:

- the contract the test is protecting,
- the current synchronization and assertion surfaces,
- whether the test uses logs, DB state, direct internals, fake first-party seams, timers, or process output,
- the smallest better seam, if the current surface is incidental.

Prefer `rg` or `sem` to locate related helpers, callers, and fixtures. Avoid broad rewrites during the audit.

## Classify Contract Type

Use one primary category per test.

- `public-daemon-behavior`: behavior observable through IPC/client calls, process lifecycle, backend collaborator calls, or daemon subscriptions.
- `persistence-guarantee`: persistence is the behavior under test, such as restart recovery, stored history, permissions, durable pull request records, or migration outcomes.
- `diagnostic-contract`: logging, redaction, auditability, crash observability, or request correlation is the explicit contract under test.
- `test-harness-infrastructure`: fixtures, daemon startup helpers, config reload harnesses, or agent test processes are the subject.
- `implementation-coupled`: the test primarily asserts internal wiring, private state, or wrapper calls without protecting an observable contract.
- `missing-seam`: the behavior matters, but the test currently relies on logs, DB rows, sleeps, or internals because no stable public, domain, or harness seam exists.

## Identify Assertion Smells

Record smells separately from the test's value. A useful test can still use the wrong surface.

- `log-proxy`: logs prove or synchronize behavior that should have a public, domain, or harness signal.
- `db-proxy`: DB rows are asserted when IPC/client queries, backend calls, or domain events would express the contract better.
- `internal-import`: tests import production internals that are not the contract under test.
- `fake-first-party`: tests fake local daemon/client wrappers or first-party modules instead of exercising the runtime path.
- `time-sensitive`: tests rely on sleeps, short timeouts, or race-prone polling.
- `multi-contract`: one test verifies unrelated behaviors or review decisions.
- `incidental-wording`: tests assert exact messages where structure or behavior would be more stable.

## Preferred Replacement Surfaces

Use the narrowest stable surface that matches the contract.

1. Public IPC/client response or follow-up query.
2. Backend harness call or delivered-event acknowledgement when the backend is the external collaborator.
3. Typed daemon event when the behavior is an operational/domain lifecycle event.
4. App or CLI output when that output is the product surface.
5. DB state only when persistence itself is the contract, or when the test is explicitly daemon integration coverage with no suitable public seam.
6. Logs only when logging, diagnostics, audit, redaction, or correlation is the explicit contract under test.

When log assertions are a proxy for behavior, prefer adding or exposing a typed daemon event. Daemon events should be capturable in tests, and operational logs should be derived automatically from event emission where practical. Avoid adding manual log calls beside events just to make tests observable.

## Audit Table Template

Create or update an audit table with these columns:

```markdown
| File:line | Test | Contract type | Contract protected | Current surfaces | Smells | Candidate event/seam | Recommendation |
|---|---|---|---|---|---|---|---|
| core/daemon/test/daemon.test.ts:120 | daemon run ... | public-daemon-behavior | Feedback events create completed sessions for matching repositories. | DB, backend harness, logs | log-proxy | Typed feedback/session lifecycle event or public session query. | Replace log waits/assertions with event or public query. |
```

Keep `Contract protected` short. It is used to decide whether the current assertion surface matches the behavior, not to restate the test body.

## Recommendation Values

Use concise, repeatable labels:

- `keep`: current assertion surface matches the contract.
- `assert-public-api`: replace implementation assertions with IPC/client responses, follow-up queries, or process behavior.
- `assert-persistence`: keep or move DB assertions because durable state is the contract.
- `replace-log-with-event`: add or expose a typed daemon event and assert that instead of logs.
- `split-diagnostic-contract`: move log assertions into tests whose explicit subject is logging, redaction, auditability, or correlation.
- `add-test-seam`: add a small harness/domain seam before refactoring assertions.
- `remove-incidental-assertion`: delete assertions that protect neither product behavior nor an explicit diagnostic contract.
- `split-test`: separate unrelated contracts into reviewable tests.

## Review Questions

Ask these before proposing refactors:

- Is this test protecting user/product behavior, persistence semantics, operator diagnostics, or only implementation wiring?
- If the assertion uses DB state, is persistence the contract, or is the DB just convenient?
- If the assertion uses logs, is the log record itself the contract, or is it standing in for a missing event/API?
- Would a typed daemon event make the behavior easier to test and also support automatic logging?
- Can the test be made less brittle without losing coverage?

## Output

An audit should produce a durable table plus a short summary:

- counts by contract type,
- counts by smell,
- proposed daemon events or harness seams,
- tests safe to refactor immediately,
- tests requiring a product, diagnostic, or architecture decision first.

Do not perform refactors in the audit change unless explicitly requested.
