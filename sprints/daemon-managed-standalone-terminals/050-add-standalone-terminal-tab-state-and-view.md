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

## Review Report

- Review question: Does the app provide a real standalone daemon-backed terminal tab with the intended open path, state retention, input/output behavior, and close/restart lifecycle?
- Approval means: The sprint has delivered the first acceptable user-visible terminal slice and can be landed or iterated from product feedback rather than infrastructure gaps.
- Downstream unlock: Final sprint acceptance and landing become possible once this behavior is accepted.
- Rework trigger: User confusion around terminal launch semantics, cwd defaults, tab close behavior, state retention, or terminal prominence would require app-state and UX revision.
- Revert or revision boundary: This app slice can be revised without changing the daemon request/stream contract or PTY runtime if the host bridge remains stable.

## Work-Ahead Safety

No further work-ahead is recommended. This is the first full vertical slice the human should accept or revise directly.
