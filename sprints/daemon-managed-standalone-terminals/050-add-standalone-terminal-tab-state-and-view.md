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

### Plain-English Summary

This task will deliver the standalone app terminal surface: a real daemon-backed terminal tab with state, output rendering, input forwarding, resize handling, close/restart behavior, and a non-debug entrypoint.

### How To Verify Without Reading Code

After implementation, review the reported app behavior checks and, if needed, inspect the app manually. Acceptance should mean a user can open multiple terminal tabs, interact with live daemon-backed terminals, preserve hidden-tab state while the Bun host remains connected, and close/restart terminals with the intended lifecycle.

### Agent Verification

- Pending implementation. Replace this with the exact automated checks and app interaction verification run before marking the task finished-unreviewed.

### Approval Questions

- Does the app provide the intended standalone terminal open path and tab behavior?
- Are cwd defaults, state retention, close semantics, and restart semantics acceptable for the first user-visible slice?
- Does the terminal remain a bounded utility rather than becoming a terminal-first primary workflow?

### Known Limits

- This is the first vertical user-visible terminal slice, not a complete terminal product.
- No additional work-ahead is recommended after this task; product feedback on the visible behavior should drive follow-up revisions.

## Work-Ahead Safety

No further work-ahead is recommended. This is the first full vertical slice the human should accept or revise directly.
