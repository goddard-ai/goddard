# 050-add-session-chat-state

Status: planned

## Title

Introduce `SessionChatState` for history, live messages, sends, and connection status

## Objective

Centralize session chat data ownership so views render from stable session state instead of direct query orchestration.

## Scope

- Add state keyed by daemon session id.
- Load latest session summary and `session.history` page.
- Subscribe to live messages while consumed and append incoming ACP messages.
- Expose send, cancel, reconnect, retry, and dispose actions where backed by current SDK methods.
- Preserve draft and send state outside presentational components.

## Dependencies

- `040-support-electrobun-session-subscriptions`
- `030-preserve-composer-draft-on-send-failure`

## Acceptance Criteria

- State is keyed by daemon session id.
- It loads the latest `session.history` page and session summary.
- It subscribes to live messages while consumed and appends incoming ACP messages.
- It exposes send, cancel, reconnect, retry, and dispose actions where backed by current SDK methods.
- It preserves draft/send state outside the presentational components.

## Constraints And Risks

- Keep the app-owned Virtuoso/Comark transcript path.
- Do not introduce broad realtime activity architecture for other domains in this task.

## Implementation Notes

Pending.

## Verification Evidence

Pending.

