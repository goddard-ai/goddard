import { useVirtualizer } from "@tanstack/react-virtual"
import { useLayoutEffect, useMemo, useRef, useState } from "preact/hooks"

import { useTabContext } from "~/tab-context.tsrx"
import styles from "./message-list.style.ts"

const initializedBottomScrollKeys = new Set<string>()

/** One visible message row handed to the row renderer. */
export type MessageListRow<Item> = {
  item: Item;
  index: number;
  viewportWidth: number;
}

/** Props accepted by the session-chat-owned virtual message list wrapper. */
export type MessageListProps<Item> = {
  items: readonly Item[];
  overscanPx?: number;
  initialScrollPosition?: "top" | "bottom";
  header?: preact.ComponentChild;
  scrollCacheKey?: string;
  preserveScrollOnPrepend?: boolean;
  loadingFallback?: preact.ComponentChild;
  defaultRowHeight?: number;
  getItemKey?: (item: Item, index: number) => string;
  estimateRowHeight?: (item: Item, index: number, viewportWidth: number) => number;
  renderRow: (row: MessageListRow<Item>) => preact.ComponentChild;
}

type MessageListViewport = {
  scrollMarginMeasured: boolean;
  scrollMargin: number;
  width: number;
}

function buildItemKeys<Item>(
  items: readonly Item[],
  getItemKey: MessageListProps<Item>["getItemKey"],
) {
  return items.map((item, index) => String(getItemKey?.(item, index) ?? index))
}

function resolvePrependedItemCount(previousKeys: readonly string[], nextKeys: readonly string[]) {
  for (const [previousIndex, previousKey] of previousKeys.entries()) {
    const nextIndex = nextKeys.indexOf(previousKey)

    if (nextIndex >= 0) {
      return Math.max(0, nextIndex - previousIndex)
    }
  }

  return 0
}

function getScrollMargin(spacerElement: HTMLDivElement | null, scrollElement: HTMLDivElement) {
  if (!spacerElement) {
    return 0
  }

  return Math.round(
    spacerElement.getBoundingClientRect().top - scrollElement.getBoundingClientRect().top +
      scrollElement.scrollTop,
  )
}

/** Renders the transcript's virtualized message list inside the nearest scrollable panel. */
export component MessageList<Item>(props: MessageListProps<Item>) {
  const rootRef = useRef<HTMLDivElement | null>(null)
  const spacerRef = useRef<HTMLDivElement | null>(null)
  const previousScrollHeightRef = useRef(0)
  const previousItemKeysRef = useRef<readonly string[]>([])
  const tabContext = useTabContext()
  const [viewport, setViewport] = useState<MessageListViewport>({
    scrollMarginMeasured: false,
    scrollMargin: 0,
    width: 0,
  })

  const scrollCacheKey = props.scrollCacheKey
  const effectiveInitialScroll = props.initialScrollPosition ?? "top"
  const overscanPx = props.overscanPx ?? 320
  const itemKeys = useMemo(() => buildItemKeys(props.items, props.getItemKey), [
    props.getItemKey,
    props.items,
  ])
  useLayoutEffect(() => {
    const rootElement = rootRef.current
    const scrollElement = tabContext.scrollParentRef.current

    if (!rootElement || !scrollElement) {
      return
    }

    const updateViewport = () => {
      const spacerElement = spacerRef.current

      setViewport({
        scrollMargin: getScrollMargin(spacerElement, scrollElement),
        scrollMarginMeasured: Boolean(spacerElement),
        width: rootElement.clientWidth,
      })
    }

    updateViewport()

    const resizeObserver = new ResizeObserver(updateViewport)
    resizeObserver.observe(rootElement)

    return () => {
      resizeObserver.disconnect()
    }
  }, [tabContext.scrollParentRef])

  useLayoutEffect(() => {
    const scrollElement = tabContext.scrollParentRef.current

    if (!scrollElement || viewport.width <= 0) {
      return
    }

    const scrollMargin = getScrollMargin(spacerRef.current, scrollElement)

    if (scrollMargin !== viewport.scrollMargin || !viewport.scrollMarginMeasured) {
      setViewport(
        (currentViewport) => ({
          ...currentViewport,
          scrollMargin,
          scrollMarginMeasured: true,
        }),
      )
    }
  })

  const virtualizer = useVirtualizer<HTMLDivElement, HTMLDivElement>({
    count: props.items.length,
    estimateSize: (index) => {
      const item = props.items[index]

      if (!item) {
        return props.defaultRowHeight ?? 1
      }

      if (props.estimateRowHeight && viewport.width > 0) {
        return Math.max(1, Math.round(props.estimateRowHeight(item, index, viewport.width)))
      }

      return props.defaultRowHeight ?? 1
    },
    getItemKey: (index) => itemKeys[index] ?? index,
    getScrollElement: () => tabContext.scrollParentRef.current,
    overscan: Math.max(1, Math.ceil(overscanPx / Math.max(1, props.defaultRowHeight ?? 1))),
    scrollMargin: viewport.scrollMargin,
    useFlushSync: false,
  })

  useLayoutEffect(() => {
    const scrollElement = tabContext.scrollParentRef.current

    if (!scrollElement) {
      return
    }

    if (props.preserveScrollOnPrepend) {
      const prependedItemCount = resolvePrependedItemCount(previousItemKeysRef.current, itemKeys)

      if (prependedItemCount > 0) {
        scrollElement.scrollTop += Math.max(
          0,
          scrollElement.scrollHeight - previousScrollHeightRef.current,
        )
      }
    }

    previousItemKeysRef.current = itemKeys
    previousScrollHeightRef.current = scrollElement.scrollHeight
  }, [itemKeys, props.preserveScrollOnPrepend, tabContext.scrollParentRef])

  useLayoutEffect(() => {
    if (
      effectiveInitialScroll !== "bottom" || props.items.length === 0 || viewport.width <= 0 ||
        !viewport.scrollMarginMeasured
    ) {
      return
    }

    const initializationKey = scrollCacheKey ?? "__default__"

    if (initializedBottomScrollKeys.has(initializationKey)) {
      return
    }

    initializedBottomScrollKeys.add(initializationKey)
    requestAnimationFrame(() => {
      virtualizer.scrollToIndex(props.items.length - 1, {
        align: "end",
      })
    })
  }, [
    effectiveInitialScroll,
    props.items.length,
    scrollCacheKey,
    viewport.scrollMarginMeasured,
    viewport.width,
    virtualizer,
  ])

  <div ref={rootRef} style={{ minHeight: "100%" }}>
    if (viewport.width <= 0) {
      {props.loadingFallback ??
        <tsrx>
          <div class={styles.loading}>"Preparing message list..."</div>
        </tsrx>}
    } else {
      if (props.header) {
        {props.header}
      }
      <div
        ref={spacerRef}
        class={styles.spacer}
        style={{
          height: `${virtualizer.getTotalSize()}px`,
        }}
      >
        {virtualizer.getVirtualItems().map((virtualItem) => {
          const item = props.items[virtualItem.index]

          if (!item) {
            return
          }

          <div
            key={virtualItem.key}
            data-index={virtualItem.index}
            ref={virtualizer.measureElement}
            class={styles.row}
            style={{
              transform: `translateY(${virtualItem.start - viewport.scrollMargin}px)`,
            }}
          >
            {props.renderRow({
              item,
              index: virtualItem.index,
              viewportWidth: viewport.width,
            })}
          </div>
        })}
      </div>
    }
  </div>
}
