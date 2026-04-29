# 090-page-older-session-history

Status: planned

## Title

Load older session history pages

## Objective

Make long session transcripts usable by loading older history pages on demand while preserving scroll position.

## Scope

- Use `nextCursor` and `hasMore` from session history.
- Load older turns when scrolling near the top.
- Avoid duplicate page loads.
- Preserve visible scroll position when older rows are prepended.

## Dependencies

- `050-add-session-chat-state`
- `060-wire-session-chat-view-to-state`
- `080-render-turn-stop-and-permission-rows`

## Acceptance Criteria

- Transcript exposes `hasOlderMessages` and loads older turns with `nextCursor`.
- Scrolling near the top can request older history without duplicate pages.
- Prepending older rows preserves the user's visible scroll position.
- Long sessions remain usable after reopening the tab.

## Constraints And Risks

- Row identity and state ownership should be stable before this task begins.
- Keep this task focused on history paging, not richer transcript row actions.

## Implementation Notes

Pending.

## Verification Evidence

Pending.

