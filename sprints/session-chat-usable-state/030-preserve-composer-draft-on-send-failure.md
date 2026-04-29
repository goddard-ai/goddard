# 030-preserve-composer-draft-on-send-failure

Status: planned

## Title

Make session prompt sends draft-safe

## Objective

Prevent users from losing prompt drafts when a session prompt send fails.

## Scope

- Preserve composer content after failed `session.prompt` calls.
- Clear and refocus the composer only after an accepted send.
- Surface visible send failure state or toast.
- Prevent duplicate sends while a prompt send is in flight.

## Dependencies

- `020-add-session-chat-load-states`

## Acceptance Criteria

- Failed `session.prompt` calls do not clear composer content.
- Successful accepted sends still clear and refocus the composer.
- Users see a visible send failure state or toast.
- Duplicate sends are blocked while a send is in flight.

## Constraints And Risks

- Draft-safe semantics must be preserved when later chat state is introduced.
- Keep composer behavior compatible with the existing session launch form use of shared input components.

## Implementation Notes

Pending.

## Verification Evidence

Pending.

