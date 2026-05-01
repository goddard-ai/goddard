# Handoff: daemon-managed-standalone-terminals

## Current Context

- Confirmed sprint plan name: `daemon-managed-standalone-terminals`.
- Base branch assumption: `codex/add-custom-terminal-input`.
- The user confirmed the sprint should include a real standalone terminal tab and app state surface, not only daemon infrastructure.
- The agreed architecture is: daemon owns PTYs, desktop Bun host owns the daemon terminal websocket connection, webview talks to Bun host through Electrobun.

## Key Constraints

- Use `sprint-branch` for branch-management transitions.
- Do not use ACP terminal capabilities for this work.
- Terminal instance ids are local to the Bun-host daemon websocket connection.
- A client can only address terminal instances it created.
- Disconnecting the Bun host from the daemon terminal websocket disposes every terminal instance for that connection.
- `bun-pty` may be used from the daemon if daemon runtime and standalone packaging validation succeeds.

## Review Notes

- `020-validate-daemon-pty-runtime` is the stop/go task for `bun-pty` viability.
- `050-add-standalone-terminal-tab-state-and-view` is the first user-visible terminal slice and should not be worked beyond until reviewed.

