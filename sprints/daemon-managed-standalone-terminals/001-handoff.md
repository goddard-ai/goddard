# Handoff: daemon-managed-standalone-terminals

## Current Context

- Confirmed sprint plan name: `daemon-managed-standalone-terminals`.
- Base branch assumption: `codex/add-custom-terminal-input`.
- The user confirmed the sprint should include a real standalone terminal tab and app state surface, not only daemon infrastructure.
- The agreed architecture is: daemon owns PTYs, desktop Bun host owns the daemon terminal stream connection, webview talks to Bun host through Electrobun.
- Sprint branch state is initialized; `010-define-terminal-contract-and-lifecycle` is complete on review and waiting for human review.
- Human feedback replaced the prior websocket transport direction with HTTP requests plus daemon streams.

## Key Constraints

- Use `sprint-branch` for branch-management transitions.
- Do not use ACP terminal capabilities for this work.
- Terminal instance ids are local to the Bun-host daemon terminal stream connection.
- A client can only address terminal instances it created.
- Disconnecting the Bun host from the daemon terminal stream disposes every terminal instance for that connection.
- `bun-pty` may be used from the daemon if daemon runtime and standalone packaging validation succeeds.

## Review Notes

- `020-validate-daemon-pty-runtime` is the stop/go task for `bun-pty` viability.
- `050-add-standalone-terminal-tab-state-and-view` is the first user-visible terminal slice and should not be worked beyond until reviewed.

## Task 010 Notes

- The terminal contract lives in shared schema as HTTP request payloads plus daemon stream events, not websocket frames.
- SDK additions are type-only surface definitions for the future `sdk.terminal` runtime.
- Focused schema and SDK tests, typechecks, and lints passed.
