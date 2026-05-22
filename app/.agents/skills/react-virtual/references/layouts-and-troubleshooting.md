# @tanstack/react-virtual layouts and troubleshooting

## Element-scroller checklist

- The scroll element has a real constrained size (`height`, `max-height`, or width for horizontal lists) and `overflow: auto`.
- `getScrollElement` returns the same DOM element the user scrolls.
- The inner spacer size is `virtualizer.getTotalSize()` on the active axis.
- Only `virtualizer.getVirtualItems()` are rendered.
- Virtual rows are positioned from `virtualItem.start` and keyed by `virtualItem.key`.

## Positioning strategies

### Absolute per item

Good for simple fixed or measured lists.

```tsx
<div style={{ height: virtualizer.getTotalSize(), position: 'relative' }}>
  {virtualizer.getVirtualItems().map((item) => (
    <div
      key={item.key}
      style={{
        position: 'absolute',
        top: 0,
        left: 0,
        width: '100%',
        height: item.size,
        transform: `translateY(${item.start}px)`,
      }}
    />
  ))}
</div>
```

### Block translation

Useful for dynamic lists and preferred for smooth scrolling because some far-away measurements are intentionally skipped around smooth scroll targets.

```tsx
const items = virtualizer.getVirtualItems()

<div style={{ height: virtualizer.getTotalSize(), position: 'relative' }}>
  <div style={{ transform: `translateY(${items[0]?.start ?? 0}px)` }}>
    {items.map((item) => (
      <div key={item.key} data-index={item.index} ref={virtualizer.measureElement} />
    ))}
  </div>
</div>
```

## Dynamic-size items

- Use `ref={virtualizer.measureElement}` on the element whose size should count.
- Add `data-index={virtualItem.index}` to measured elements.
- Estimate a realistic upper-ish size. Underestimation causes more scroll correction.
- Avoid CSS margins that collapse or protrude outside the measured element; prefer padding or `gap`.
- If item content changes size after render, ensure ResizeObserver can see the measured element change.
- Do not mix `measureElement` and `resizeItem` for the same indexes.

## Window virtualizer

Use `useWindowVirtualizer` when the page scrolls. Common requirements:

- Account for the list's offset from the top of the document.
- Use `scrollMargin` when content starts below a header or other dynamic top content.
- Subtract `virtualizer.options.scrollMargin` in absolute transforms.
- Use `initialOffset` and `initialMeasurementsCache` when restoring a route to avoid remeasuring and jumping.

## Horizontal lists

Set `horizontal: true`, size the spacer with width, and map offsets to `translateX`/`left`. Use `isRtl` for right-to-left horizontal scrolling.

## Grids and masonry

- For strict row/column grids, combine row and column virtualizers.
- For masonry-like layouts, use `lanes`. Vertical virtualizer lanes are columns; horizontal virtualizer lanes are rows.
- Prefer default `laneAssignmentMode: 'estimate'` if visual stability matters; use `'measured'` when accurate measured lane packing matters more than initial stability.

## Sticky rows

Use `rangeExtractor` to include sticky indexes in addition to the visible range. Keep sticky row identity separate from normal rows and inspect `examples/sticky/src/main.tsx` before implementing.

## Infinite loading

- Use the virtual range to detect when the last rendered item approaches loaded data length.
- Keep `count` aligned with loaded rows plus any loading sentinel.
- Use stable keys so appended pages do not invalidate existing rows.
- Inspect `examples/infinite-scroll/src/main.tsx` for a small reference implementation.

## Scroll positioning

- Use `scrollToIndex` for item targets and `scrollToOffset` / `scrollBy` for pixel targets.
- Use `scrollPaddingStart` and `scrollPaddingEnd` for fixed headers or desired alignment breathing room.
- Smooth scrolling works best with fixed sizes or block translation. Dynamic measurement can otherwise shift the target while animation is running.

## Debugging symptoms

- **No scrollbar or all rows overlap:** spacer is missing `getTotalSize()`, scroll container has no constrained size, or item transform ignores `start`.
- **Rows render but scrolling is page-owned unexpectedly:** use `useWindowVirtualizer`, or move `overflow: auto` and height to the element returned by `getScrollElement`.
- **Scroll jumps with dynamic rows:** improve `estimateSize`, measure the correct element, avoid margins, and check `shouldAdjustScrollPositionOnItemSizeChange` only after layout fixes.
- **Wrong row content after insert/reorder:** add `getItemKey` based on stable data IDs.
- **Sticky rows disappear:** include sticky indexes via `rangeExtractor`; rendering only visible indexes will drop them.
- **React 19 warning about `flushSync`:** set `useFlushSync: false` unless synchronous scroll rendering is required.
- **ResizeObserver loop warning:** first remove resize feedback loops; only then consider `useAnimationFrameWithResizeObserver` as a workaround.
