# 100-render-permission-request-rows

Status: planned

## Title

Render ACP permission request rows

## Objective

Surface permission requests in the transcript so blocked sessions are actionable and understandable.

## Scope

- Normalize ACP permission requests into transcript rows.
- Render request context and available approve or deny choices.
- Wire responses only to the existing supported permission response mechanism.
- Preserve visible resolved, denied, canceled, or failed outcomes after response.

## Dependencies

- `050-add-session-chat-state`
- `060-wire-session-chat-view-to-state`
- `080-add-session-chat-header-actions`
- `090-render-turn-stop-rows`

## Acceptance Criteria

- Permission requests show requested action and relevant target or context.
- Approve and deny controls appear only while a request is actionable.
- Resolved, denied, canceled, or failed outcomes remain visible after response.
- Response failures are visible and do not silently mark the request resolved.
- Focused validation covers pending, resolved, and failure states.

## Review Checkpoint

Confirm security-sensitive permission wording and action semantics.

## Work-Ahead Safety

Do not work ahead from this task until reviewed because permission UX and response semantics are security-sensitive.

## Constraints And Risks

- Do not invent app-only permission contracts when an ACP or SDK contract exists.
- Keep permission responses scoped to the current session.

## Implementation Notes

Pending.

## Verification Evidence

Pending.
