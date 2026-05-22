import { useVirtualizer, type VirtualItem } from "@tanstack/react-virtual"
import { useLayoutEffect, useMemo, useRef, useState } from "preact/hooks"

import { useTabContext } from "~/tab-context.tsrx"
import styles from "./message-list.style.ts"

const initializedBottomScrollKeys = new Set<string>()
const messageListScrollCache = new Map<
  string,
  {
    itemKeys: readonly string[];
    measurements: readonly VirtualItem[];
    scrollTop: number;
  }
>()

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

function getScrollMargin(spacerElement: HTMLDivElement | null, scrollElement: HTMLDivElement) {
  if (!spacerElement) {
    return 0
  }

  return Math.round(
    spacerElement.getBoundingClientRect().top - scrollElement.getBoundingClientRect().top +
      scrollElement.scrollTop,
  )
}

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
  const effectiveScrollCacheKey = scrollCacheKey ?? tabContext.id
  const effectiveInitialScroll = props.initialScrollPosition ?? "top"
  const overscanPx = props.overscanPx ?? 320
  const itemKeys = useMemo(
    () => props.items.map((item, index) => String(props.getItemKey?.(item, index) ?? index)),
    [props.getItemKey, props.items],
  )
  const cachedScrollSnapshot = messageListScrollCache.get(effectiveScrollCacheKey)
  const restoredScrollSnapshot =
    cachedScrollSnapshot &&
    cachedScrollSnapshot.itemKeys.every((itemKey, index) => itemKeys[index] === itemKey)
      ? cachedScrollSnapshot
      : undefined

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
    enabled: viewport.width > 0,
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
    initialMeasurementsCache: restoredScrollSnapshot
      ? [...restoredScrollSnapshot.measurements]
      : undefined,
    initialOffset: restoredScrollSnapshot?.scrollTop,
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
      let prependedItemCount = 0

      for (const [previousIndex, previousKey] of previousItemKeysRef.current.entries()) {
        const nextIndex = itemKeys.indexOf(previousKey)

        if (nextIndex >= 0) {
          prependedItemCount = Math.max(0, nextIndex - previousIndex)
          break
        }
      }

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
    return () => {
      const scrollElement = tabContext.scrollParentRef.current

      if (scrollElement) {
        messageListScrollCache.set(effectiveScrollCacheKey, {
          itemKeys,
          measurements: virtualizer.measurementsCache.map(
            (measurement) => ({
              ...measurement,
            }),
          ),
          scrollTop: scrollElement.scrollTop,
        })
      }
    }
  }, [effectiveScrollCacheKey, itemKeys, tabContext.scrollParentRef, virtualizer])

  useLayoutEffect(() => {
    if (
      effectiveInitialScroll !== "bottom" || props.items.length === 0 || viewport.width <= 0 ||
        !viewport.scrollMarginMeasured
    ) {
      return
    }

    if (
      initializedBottomScrollKeys.has(effectiveScrollCacheKey) ||
        messageListScrollCache.has(effectiveScrollCacheKey)
    ) {
      return
    }

    initializedBottomScrollKeys.add(effectiveScrollCacheKey)
    requestAnimationFrame(() => {
      virtualizer.scrollToIndex(props.items.length - 1, {
        align: "end",
      })
    })
  }, [
    effectiveInitialScroll,
    effectiveScrollCacheKey,
    props.items.length,
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

          return <tsrx>
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
          </tsrx>
        })}
      </div>
    }
  </div>
}
