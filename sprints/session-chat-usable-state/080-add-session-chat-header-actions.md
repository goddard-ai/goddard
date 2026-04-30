# 080-add-session-chat-header-actions

Status: planned

## Title

Add session chat header actions

## Objective

Add the first daily-use header actions that operate on the current session.

## Scope

- Add narrowly scoped actions such as reconnect, retry, stop or cancel when active, and open related changes when available.
- Show actions only when valid for the current session state.
- Surface action failures visibly.

## Dependencies

- `070-render-session-chat-header-status`

## Acceptance Criteria

- Actions appear only when valid for the current session state.
- Stop or cancel invokes the existing supported session control path when available.
- Reconnect or retry invokes the existing supported recovery path when available.
- Open changes opens the session changes tab with its required typed payload.
- Action failures are visible and do not silently leave stale UI state.

## Review Checkpoint

Confirm action availability, labels, and failure behavior.

## Work-Ahead Safety

Do not work ahead into permission handling if stop or cancel semantics are disputed because both depend on session control semantics.

## Constraints And Risks

- Do not add broad action catalogs or future PR review workflows.
- Keep actions aligned with currently available SDK and app surfaces.

## Implementation Notes

Pending.

## Verification Evidence

Pending.
