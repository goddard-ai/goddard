# 010-pass-workbench-tab-payloads

Status: planned

## Title

Pass detail-tab payloads into tab components

## Objective

Make registered detail-tab components receive their stored tab payloads so `SessionChatView` and related detail surfaces can render the correct session.

## Scope

- Pass active detail-tab payload props from the workbench tab renderer into the registered tab component.
- Preserve existing primary workbench rendering behavior.
- Ensure invalid or missing payloads fail gracefully enough to avoid a blank workbench.

## Dependencies

None.

## Acceptance Criteria

- `SessionChatView` receives `sessionId` when opened from the session list and launch dialog.
- Existing detail tabs that rely on payloads, such as session changes, still receive their props.
- Invalid or missing payloads fail gracefully instead of throwing a blank workbench.

## Constraints And Risks

- All later chat work depends on reliable tab identity.
- Keep this change narrowly focused on payload delivery and immediate safety behavior.

## Implementation Notes

Pending.

## Verification Evidence

Pending.

