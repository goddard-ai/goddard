# 090-render-turn-stop-rows

Status: planned

## Title

Render ACP turn stop rows

## Objective

Make completed, stopped, failed, and interrupted turns visible in the transcript.

## Scope

- Normalize ACP turn stop updates into transcript rows.
- Render compact turn stop rows with status, reason, and timestamp where available.
- Preserve existing user, assistant, resource link, and tool-call rows.

## Dependencies

- `050-add-session-chat-state`
- `060-wire-session-chat-view-to-state`

## Acceptance Criteria

- Turn endings no longer disappear or render as unsupported rows.
- Completed, stopped, failed, and interrupted states are visually distinct.
- Existing user, assistant, resource link, and tool-call rows continue to render.
- Focused normalization tests cover representative stop states.

## Review Checkpoint

Confirm lifecycle row wording and visual treatment.

## Work-Ahead Safety

Safe to work one task ahead on permission rows because permission rendering is a separate ACP row type.

## Constraints And Risks

- Preserve ACP shapes rather than inventing app-only message contracts.
- Do not combine permission request behavior into this task.

## Implementation Notes

Pending.

## Verification Evidence

Pending.
