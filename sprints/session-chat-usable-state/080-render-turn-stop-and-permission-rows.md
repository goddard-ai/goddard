# 080-render-turn-stop-and-permission-rows

Status: planned

## Title

Render daily-use ACP lifecycle rows

## Objective

Make the transcript represent ACP turn completion and permission flow clearly enough for normal coding-agent sessions.

## Scope

- Add transcript normalization for `turnStop` rows.
- Add inline permission request rows with available choices.
- Preserve visible permission outcomes after resolution or cancellation.
- Keep existing user, assistant, resource link, and tool-call rows working.
- Add focused normalization tests.

## Dependencies

- `050-add-session-chat-state`
- `060-wire-session-chat-view-to-state`

## Acceptance Criteria

- Transcript normalization includes `turnStop` rows for ACP stop reasons.
- Permission requests render inline with available choices.
- Permission outcomes remain visible after resolution or cancellation.
- Existing user, assistant, resource link, and tool-call rows continue to render.
- Focused normalization tests cover stop states and permission lifecycle behavior.

## Constraints And Risks

- Preserve ACP shapes rather than inventing app-only message contracts.
- Pause for close review after this task because it changes protocol normalization.

## Implementation Notes

Pending.

## Verification Evidence

Pending.

