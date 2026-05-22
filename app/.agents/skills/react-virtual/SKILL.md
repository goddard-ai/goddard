---
name: react-virtual
description: Build and debug React virtualization with @tanstack/react-virtual lists, window scrollers, grids, tables, sticky rows, infinite loading, measurement, and scroll positioning.
---

# react-virtual

Use this skill when a React codebase uses, adds, or debugs `@tanstack/react-virtual` / TanStack Virtual.

TanStack Virtual is headless: it never renders markup or styles for you. The virtualizer supplies item indexes, sizes, offsets, scroll state, and imperative methods; the app owns the scroll container, spacer element, row markup, transforms, and CSS.

## Core mental model

A correct virtualized list has four pieces:

1. A real scroll owner: either an element used with `useVirtualizer`, or `window` used with `useWindowVirtualizer`.
2. A virtualizer configured with `count`, `getScrollElement`, and `estimateSize`.
3. A spacer sized from `virtualizer.getTotalSize()` so the browser has the right scrollbar range.
4. Rows rendered from `virtualizer.getVirtualItems()` and positioned from each `VirtualItem.start`.

`VirtualItem` fields matter in rendering:

- `key`: use as the React key; prefer a data ID via `getItemKey` over index keys for mutable data.
- `index`: lookup into the app's data.
- `start`: pixel offset on the active axis; usually maps to `translateY` or `translateX`.
- `size`: estimated or measured item size.
- `lane`: column/row lane for masonry-style layouts; `0` for regular lists.

## Start Here

- Inspect the current virtualizer before editing: hook used, scroll owner, item count, `estimateSize`, `getItemKey`, spacer sizing, item positioning styles, and whether rows are dynamically measured.
- For installation or first integration, read [setup.md](./references/setup.md).
- For options, methods, and `VirtualItem` fields, read [options-and-api.md](./references/options-and-api.md).
- For layout patterns and debugging, read [layouts-and-troubleshooting.md](./references/layouts-and-troubleshooting.md).
- For runnable source patterns, inspect the bundled examples:
  - `examples/fixed/src/main.tsx` for fixed-size element scrolling.
  - `examples/dynamic/src/main.tsx` and `examples/variable/src/main.tsx` for measured or variable-size items.
  - `examples/window/src/main.tsx` for window scrolling.
  - `examples/infinite-scroll/src/main.tsx` for load-more lists.
  - `examples/table/src/main.tsx` for virtualized table rows.
  - `examples/sticky/src/main.tsx` for sticky rows with a custom range.
  - `examples/padding/src/main.tsx` and `examples/scroll-padding/src/main.tsx` for visual and scroll alignment padding.
  - `examples/smooth-scroll/src/main.tsx` for imperative smooth scrolling constraints.

## Default implementation guidance

- Use `useVirtualizer` for an element scroller. Give the scroll element a constrained height/width and `overflow: auto`.
- Use `useWindowVirtualizer` only when the page/window owns scrolling; account for the list's document offset and use `scrollMargin` when content above the list affects offsets.
- Keep the spacer size tied to `getTotalSize()`; missing or stale spacer size is a common cause of broken scrollbars.
- Render only virtual items, not the whole data array.
- Position vertical rows with `translateY(virtualItem.start)` and horizontal rows with `translateX(virtualItem.start)`.
- Provide `getItemKey` for data-backed lists that can be inserted, removed, sorted, filtered, or reordered.
- Start with `overscan` before adding custom rendering ranges; use `rangeExtractor` only for sticky/header/footer indexes that must render outside the visible range.

## Dynamic measurement rules

- For variable-height/width rows, attach `ref={virtualizer.measureElement}` to the measured element and set `data-index={virtualItem.index}`.
- Choose a realistic upper-ish `estimateSize`; underestimates create more scroll correction and visible jumping.
- Prefer padding or the virtualizer `gap` option over margins that protrude outside the measured item.
- Do not use `resizeItem` and `measureElement` on the same indexes.
- Smooth scrolling is most reliable with fixed sizes. With dynamic sizes, prefer block translation from the first virtual item's start so skipped far-away measurements do not break the animation.

## Troubleshooting shortcuts

- No scrollbar: verify scroll container sizing/overflow and spacer `getTotalSize()`.
- Rows overlap: verify absolute/block positioning uses `VirtualItem.start` and the active axis.
- Wrong row after reorder: add stable `getItemKey`.
- Scroll jumps with dynamic rows: improve `estimateSize`, measure the actual row, remove protruding margins, then consider `shouldAdjustScrollPositionOnItemSizeChange` only if needed.
- Sticky item disappears: include it with `rangeExtractor`.
- React 19 `flushSync` warning: consider `useFlushSync: false`.
- ResizeObserver loop warning: fix resize feedback loops first; `useAnimationFrameWithResizeObserver` is only a measured workaround.

## Decision Rules

- Read [setup.md](./references/setup.md) before adding the package or creating a first virtualized list.
- Read [options-and-api.md](./references/options-and-api.md) before using less-common options such as `rangeExtractor`, `lanes`, `scrollMargin`, `initialMeasurementsCache`, custom observers, custom `scrollToFn`, `resizeItem`, or `shouldAdjustScrollPositionOnItemSizeChange`.
- Read [layouts-and-troubleshooting.md](./references/layouts-and-troubleshooting.md) before changing item measurement, scroll ownership, row positioning, sticky behavior, grids/masonry, infinite loading, or scroll restoration.
- Inspect the matching `examples/*/src/main.tsx` before implementing fixed, dynamic, variable, infinite, sticky, table, smooth-scroll, padding, or window-scroller behavior.
