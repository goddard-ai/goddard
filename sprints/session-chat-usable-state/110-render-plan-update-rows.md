# 110-render-plan-update-rows

Status: planned

## Title

Render ACP plan update rows

## Objective

Show agent plan updates in the transcript instead of dropping them or treating them as unsupported events.

## Scope

- Normalize ACP plan update events into transcript rows.
- Render current plan state in a compact inline panel.
- Avoid confusing duplicates when repeated updates replace or advance the same plan.

## Dependencies

- `050-add-session-chat-state`
- `060-wire-session-chat-view-to-state`
- `090-render-turn-stop-rows`
- `100-render-permission-request-rows`

## Acceptance Criteria

- Plan updates render in transcript order.
- Step status is readable and updates coherently across repeated plan updates.
- Repeated updates do not create confusing duplicates.
- Focused validation covers representative plan events.

## Review Checkpoint

Confirm whether plan updates belong inline in the transcript and how much detail they should show.

## Work-Ahead Safety

Safe to work one task ahead on history paging after row identity rules are accepted.

## Constraints And Risks

- Agent-thought visibility remains out of scope unless explicitly added.
- Preserve transcript row identity so later history paging can prepend older rows safely.

## Implementation Notes

Pending.

## Verification Evidence

Pending.
