# Scrolling And Performance

Use this reference when the task is about load-more behavior, initial positioning, autoscroll, rendering gaps, or jank.

## Loading And Positioning

- Use `endReached` for forward infinite scrolling.
- Use `firstItemIndex` with prepended data for inverse infinite scrolling. Keep `firstItemIndex` positive and consistent with the total data span.
- Use `initialTopMostItemIndex` for the initial position. Prefer it over `initialScrollTop` when possible.
- Use imperative `scrollToIndex` after mount when the position must change later.
- Use `alignToBottom` for short chat histories that should sit on the bottom edge.
- Use `followOutput` for append-at-bottom feeds. Prefer the function form when autoscroll depends on whether the user is already at the bottom.
- Use `atTopStateChange`, `atBottomStateChange`, `atTopThreshold`, and `atBottomThreshold` for load triggers and unread-state logic.
- Use `useWindowScroll` when the page should own scrolling instead of an inner div.
- Use `customScrollParent` when an existing container should own scrolling.

## Rendering Tuning

- Start with the default variable-height behavior.
- Set `fixedItemHeight` only when row heights are truly fixed.
- Set `fixedGroupHeight` only together with `fixedItemHeight` when group headers are also fixed.
- Set `defaultItemHeight` when the first rendered item is an outlier and probe-based sizing slows initial fill.
- Use `heightEstimates` when heights vary widely and rough per-item estimates are available.
- Increase `increaseViewportBy` when heavy content creates blank gaps during scrolling.
- Use `minOverscanItemCount` for very tall or dynamic rows when pixel-based viewport growth is not enough.
- Use `overscan` to render in larger chunks and reduce rerenders during scroll.
- Use `scrollSeekConfiguration` with `components.ScrollSeekPlaceholder` to swap in lightweight placeholders while the user scrolls quickly.
- Use `isScrolling` to swap expensive content for cheaper placeholders during active scroll.

## Practical Heuristics

- Reduce viewport size first when performance is poor. Rendering fewer visible rows is often the biggest win.
- Memoize expensive item subcomponents when rerender cost is high, but do not blanket-memoize the whole tree.
- Treat `rangeChanged` as the rendered range, not necessarily the strictly visible range, because overscan and viewport expansion affect it.
