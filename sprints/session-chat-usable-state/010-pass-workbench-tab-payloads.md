# 010-pass-workbench-tab-payloads

Status: finished-unreviewed

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

- Started on `sprint/session-chat-usable-state/review`.
- `AppShellWorkbenchContent` now spreads active detail-tab payloads into the registered tab component.
- `SessionChatView` renders a small missing-session-id state before mounting the connected data-loading view.
- `SessionChangesView` keeps `sessionId` as a required prop; the valid render path always opens it from a session row with a concrete session id.
- Review feedback removed an over-defensive missing-payload fallback from `SessionChangesView`.

## Verification Evidence

- `bun run typecheck` in `app/` passed.
- `bun run test` in `app/` passed: 36 tests, 82 assertions.
