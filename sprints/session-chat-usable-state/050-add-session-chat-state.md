# 050-add-session-chat-state

Status: planned

## Title

Introduce `SessionChatState` for history, live messages, sends, and connection status

## Objective

Centralize session chat data ownership so views render from stable session state instead of direct query orchestration and consume the existing daemon stream bridge for live updates.

## Scope

- Add state keyed by daemon session id.
- Load latest session summary and `session.history` page.
- Subscribe to `session.message` while consumed and append incoming ACP messages through the existing daemon subscription bridge.
- Expose send, cancel, reconnect, retry, and dispose actions where backed by current SDK methods.
- Preserve draft and send state outside presentational components.

## Dependencies

- `030-preserve-composer-draft-on-send-failure`

## Acceptance Criteria

- State is keyed by daemon session id.
- It loads the latest `session.history` page and session summary.
- It subscribes to `session.message` through the existing daemon stream bridge and appends incoming ACP messages.
- It exposes send, cancel, reconnect, retry, and dispose actions where backed by current SDK methods.
- It preserves draft/send state outside the presentational components.

## Review Checkpoint

Confirm the state contract before the view is rewritten around it.

## Work-Ahead Safety

Do not work ahead into `060-wire-session-chat-view-to-state` until this is reviewed because UI wiring would bake in the state API.

## Constraints And Risks

- Keep the app-owned Virtuoso/Comark transcript path.
- Do not introduce broad realtime activity architecture for other domains in this task.
- The generic daemon stream bridge exists in `d7e87aa4b`; do not rebuild it here.

## Implementation Notes

Pending.

## Verification Evidence

Pending.
