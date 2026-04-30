# 010-require-session-tab-payloads

Status: planned

## Title

Require session tab payloads at the type level

## Objective

Make session detail tabs impossible to open without their required session payload data in normal typed code paths.

## Scope

- Make `SessionChatView` require `sessionId` in its props.
- Enforce the session chat tab payload shape through the workbench tab definition and opening API.
- Keep `SessionChangesView` requiring `sessionId`, `sessionTitle`, and `repositoryLabel`.
- Preserve a boundary-level strategy only for persisted or corrupt tab data that cannot be proven by TypeScript.

## Dependencies

None.

## Acceptance Criteria

- Typed callers cannot open a `sessionChat` tab without `sessionId`.
- `SessionChatView` no longer accepts `undefined` or `null` for `sessionId`.
- `SessionChangesView` remains type-required for its payload data.
- Runtime fallback, if still needed for corrupt persisted tab data, lives at the workbench tab boundary rather than inside the session chat component.
- App typecheck passes.

## Review Checkpoint

Confirm the typed tab payload contract and where any unavoidable corrupt-persistence fallback belongs.

## Work-Ahead Safety

Safe to work one task ahead after this because later chat work depends only on the stable required `sessionId` boundary.

## Constraints And Risks

- Avoid treating missing required tab data as a valid component state.
- Keep the change focused on tab typing and payload delivery, not broader session chat behavior.

## Implementation Notes

Pending.

## Verification Evidence

Pending.
