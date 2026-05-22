# @tanstack/react-virtual options and API

## React hooks

- `useVirtualizer(options)`: element-owned scrolling. Provide `getScrollElement`.
- `useWindowVirtualizer(options)`: window-owned scrolling. The adapter configures the window scroll element and observers.
- React option `useFlushSync` defaults to `true`. Set `false` for React 19 `flushSync` warnings, tests, lower-end device performance, or lists where a small scroll-render delay is acceptable.

## Required options

- `count`: total number of virtual items.
- `getScrollElement`: returns the scroll container. It may return `null` before mount.
- `estimateSize(index)`: estimated size on the active axis. When dynamically measuring, estimate the largest realistic size to reduce initial scroll correction.

## Common options

- `getItemKey(index)`: return a stable item key. Use data IDs for mutable/reordered data instead of default index keys. Memoize it in React when practical.
- `overscan`: extra items above/below the visible range. Default is `1`. Increase to avoid blanking during fast scroll; decrease to reduce DOM/render cost.
- `horizontal`: switches the active axis to width/`translateX`.
- `paddingStart` / `paddingEnd`: visual padding inside the virtualized range.
- `scrollPaddingStart` / `scrollPaddingEnd`: offsets used by `scrollToIndex` / `scrollToOffset` alignment.
- `initialOffset`: starting scroll offset, useful for SSR, conditional mounting, and restoring position.
- `initialRect`: seed scroll element dimensions before real measurement, mainly SSR.
- `enabled`: set `false` to disable observers and reset virtualizer state while hidden/inactive.
- `onChange(instance, sync)`: state-change callback. `sync` is true during active scrolling and false after scroll stops or non-scroll updates.
- `debug`: logs virtualizer internals.

## Advanced layout options

- `rangeExtractor(range)`: customize which indexes render. Use for sticky items, headers, or footers that must render outside the visible range. Start from `defaultRangeExtractor` unless you need full control.
- `scrollMargin`: changes where the scroll offset originates. Common with `useWindowVirtualizer` when content starts below a header. Subtract it in transforms, e.g. `translateY(virtualRow.start - virtualizer.options.scrollMargin)`.
- `gap`: pixel spacing between virtual items without manual per-row margin math.
- `lanes`: masonry-style multi-lane layout. For vertical virtualizers, lanes are columns; for horizontal virtualizers, lanes are rows.
- `laneAssignmentMode`: `'estimate'` caches lane assignment immediately from estimated sizes to prevent jumping; `'measured'` waits for actual measurement for better lane choices but may change initial assignment.
- `isRtl`: invert horizontal scrolling for right-to-left locales.

## Measurement options

- `measureElement(element, entry, instance)`: custom size measurement function. The default uses `getBoundingClientRect()`.
- `useAnimationFrameWithResizeObserver`: normally leave `false`. It defers ResizeObserver processing to the next animation frame, adding delay; use only for measured workarounds such as ResizeObserver loop errors after diagnosing the root cause.
- `shouldAdjustScrollPositionOnItemSizeChange(item, delta, instance)`: override scroll correction for size changes above the viewport. The default avoids scroll-up jank by not correcting while scrolling backward. Use sparingly.

## Scrolling options

- `scrollToFn(offset, options, instance)`: custom scroll implementation. Built-ins are configured by the React hooks.
- `isScrollingResetDelay`: debounce duration before `isScrolling` resets when not using native `scrollend`. Default is 150 ms.
- `useScrollendEvent`: opt into native `scrollend`; default is false because cross-browser support varies.

## Instance methods and fields

- `getVirtualItems()`: current `VirtualItem[]` to render.
- `getVirtualIndexes()`: current indexes to render.
- `getTotalSize()`: total virtual size in pixels. Use this for the spacer size.
- `scrollToIndex(index, { align, behavior })`: scroll to an item. `align`: `'start' | 'center' | 'end' | 'auto'`.
- `scrollToOffset(offset, { align, behavior })`: scroll to a pixel offset.
- `scrollBy(delta, { behavior })`: relative pixel scroll.
- `measure()`: reset item measurements.
- `measureElement(el)`: ref callback for dynamic measurement; add `data-index` to the element.
- `resizeItem(index, size)`: manually set an item size. Do not use it on the same indexes also observed by `measureElement`.
- `takeSnapshot()`: returns measured `VirtualItem`s. Pair with `scrollOffset` and restore using `initialMeasurementsCache` plus `initialOffset` after navigation.
- `scrollRect`, `scrollOffset`, `scrollDirection`, `isScrolling`, `scrollElement`, and read-only `options` expose current state.

## VirtualItem fields

- `key`: stable key, from `getItemKey` or index by default.
- `index`: item index.
- `start`: start offset in pixels; map to `translateY`, `translateX`, `top`, or `left`.
- `end`: end offset in pixels.
- `size`: measured or estimated size.
- `lane`: lane index; always `0` for regular one-lane lists.
