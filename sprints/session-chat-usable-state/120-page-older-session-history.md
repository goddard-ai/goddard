# 120-page-older-session-history

Status: planned

## Title

Load older session history pages

## Objective

Make long session transcripts usable by loading older history pages on demand while preserving scroll position.

## Scope

- Use `nextCursor` and `hasMore` from session history.
- Load older turns when scrolling near the top or when the user requests older history.
- Avoid duplicate page loads and duplicate rows.
- Preserve visible scroll position when older rows are prepended.

## Dependencies

- `050-add-session-chat-state`
- `060-wire-session-chat-view-to-state`
- `090-render-turn-stop-rows`
- `100-render-permission-request-rows`
- `110-render-plan-update-rows`

## Acceptance Criteria

- Transcript exposes older-history availability and can request older turns with `nextCursor`.
- Older history loads do not duplicate pages or rows.
- Prepending older rows preserves the user's visible scroll position.
- Loading and error states for older history are visible.
- Long sessions remain usable after reopening the tab.

## Review Checkpoint

Confirm history paging behavior and scroll ergonomics.

## Work-Ahead Safety

Final queue item; no work-ahead needed.

## Constraints And Risks

- Row identity and state ownership should be stable before this task begins.
- Keep this task focused on history paging, not richer transcript row actions.

## Implementation Notes

Pending.

## Verification Evidence

Pending.
