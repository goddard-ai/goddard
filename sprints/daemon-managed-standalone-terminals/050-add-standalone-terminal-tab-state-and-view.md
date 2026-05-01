# Task: 050-add-standalone-terminal-tab-state-and-view

## Status

planned

## Objective

Replace the debug-only terminal path with a real daemon-backed standalone terminal surface in the app.

## Scope

- Implement terminal session state.
- Register the real terminal tab.
- Wire toolbar/view behavior, terminal create/open flow, viewport input/resize forwarding, output rendering, close behavior, and restart behavior.
- Add the initial non-debug entrypoint for opening a terminal tab.

## Dependencies

- `040-add-bun-host-terminal-bridge` is accepted and stable enough for app state to treat terminal instances as Bun-host-backed resources.

## Acceptance Criteria

- A user can open a real standalone terminal detail tab in the app.
- The terminal tab shows live daemon-backed output, accepts input, resizes correctly, and closes/restarts correctly.
- Multiple terminal tabs can coexist and remain independently addressable.
- Cached or hidden tabs preserve terminal state while the Bun host remains connected.
- Closing the tab tears down that terminal instance; disconnecting the Bun host tears down all of its terminal instances.
- The terminal surface remains a bounded utility and does not introduce a terminal-first primary workflow.

## Review Checkpoint

The human is reviewing the actual user-visible terminal behavior, especially open-path semantics, default cwd behavior, and tab-close lifecycle.

## Work-Ahead Safety

No further work-ahead is recommended. This is the first full vertical slice the human should accept or revise directly.

