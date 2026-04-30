# 070-render-session-chat-header-status

Status: planned

## Title

Render session chat header status

## Objective

Add a useful chat header that identifies the session and shows lifecycle and connection status.

## Scope

- Render title, project or repository context, lifecycle state, blocked reason when available, and live connection state.
- Keep the header display-only in this task.
- Use existing app UI and status vocabulary patterns.

## Dependencies

- `050-add-session-chat-state`
- `060-wire-session-chat-view-to-state`

## Acceptance Criteria

- Header shows the current session title and project or repository context.
- Header status updates across load, active, blocked, completed, disconnected, and error states when those states are available.
- Header layout works in normal and narrow tab widths.
- Focused app validation passes.

## Review Checkpoint

Confirm the status vocabulary, density, and placement before actions depend on it.

## Work-Ahead Safety

Safe to work one task ahead on header actions only if the status vocabulary is accepted; otherwise pause because action availability depends on those states.

## Constraints And Risks

- Do not add lifecycle actions in this task.
- Do not pull full action catalog, PR review, terminal replay, or browser preview work into this sprint.

## Implementation Notes

Pending.

## Verification Evidence

Pending.
