# 060-wire-session-chat-view-to-state

Status: planned

## Title

Move `SessionChatView` onto `SessionChatState`

## Objective

Make `SessionChatView` presentational over state-owned history, live updates, send state, and lifecycle actions.

## Scope

- Replace direct session/history query orchestration in `SessionChatView`.
- Render transcript and composer from state-derived props and actions.
- Preserve tab/project context reporting.
- Ensure live chunks and tool updates appear without manual query invalidation.

## Dependencies

- `050-add-session-chat-state`

## Acceptance Criteria

- `SessionChatView` no longer directly owns session/history query orchestration.
- Transcript and composer render from state-derived props/actions.
- Live agent chunks and tool updates appear without manual query invalidation or another prompt.
- Tab/project context reporting still works.

## Review Checkpoint

Confirm the first vertical slice of state-backed chat behavior.

## Work-Ahead Safety

Safe to work one task ahead on header status after `050-add-session-chat-state` is approved because header status consumes the same reviewed state contract.

## Constraints And Risks

- Keep the view mostly presentational.
- Do not expand the task into new transcript row types or header controls.

## Implementation Notes

Pending.

## Verification Evidence

Pending.
