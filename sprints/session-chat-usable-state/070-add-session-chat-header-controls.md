# 070-add-session-chat-header-controls

Status: planned

## Title

Add the session chat header and lifecycle actions

## Objective

Expose session identity, lifecycle state, connection mode, and common session actions at the top of the chat tab.

## Scope

- Implement `SessionChatHeader`.
- Show title, project or repository, display status, blocked reason, and connection mode.
- Add actions for cancel or stop, reconnect or retry, and open changes.
- Keep contextual action and PR hooks narrow or deferred if required surfaces are unavailable.

## Dependencies

- `050-add-session-chat-state`
- `060-wire-session-chat-view-to-state`

## Acceptance Criteria

- Header shows title, project/repository, display status, blocked reason, and connection mode.
- Header can cancel/stop the active turn when supported.
- Header can reconnect or retry load when possible.
- Header can open the session changes tab.
- Contextual action/PR hooks remain scoped placeholders unless the required downstream surfaces already exist.

## Constraints And Risks

- Do not pull full action catalog, PR review, terminal replay, or browser preview work into this task.
- Keep header actions aligned with currently available SDK and app surfaces.

## Implementation Notes

Pending.

## Verification Evidence

Pending.

