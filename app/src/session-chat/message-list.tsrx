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
  itemNavigation?: {
    direction: "next" | "previous";
    key: number | string;
    matchesItem: (item: Item, index: number) => boolean;
  };
  scrollCacheKey?: string;
  stickToBottomKey?: number | string;
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

function isScrolledNearBottom(scrollElement: HTMLDivElement) {
  return scrollElement.scrollHeight - scrollElement.scrollTop - scrollElement.clientHeight <= 8
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

/** Finds the row to scroll to for a direction-aware navigation request. */
function findNavigationTargetIndex<Item>(input: {
  currentOffset: number;
  defaultRowHeight: number;
  direction: "next" | "previous";
  items: readonly Item[];
  matchesItem: (item: Item, index: number) => boolean;
  measurements: readonly VirtualItem[];
}) {
  const targetIndexes = input.items.flatMap(
    (item, index) => input.matchesItem(item, index) ? [index] : [],
  )

  if (targetIndexes.length === 0) {
    return null
  }

  const getItemStart = (index: number) => input.measurements[index]?.start ??
    index * input.defaultRowHeight
  const scrollThreshold = 8

  if (input.direction === "previous") {
    for (let index = targetIndexes.length - 1; index >= 0; index -= 1) {
      const targetIndex = targetIndexes[index]

      if (
        targetIndex !== undefined &&
          getItemStart(targetIndex) < input.currentOffset - scrollThreshold
      ) {
        return targetIndex
      }
    }

    return null
  }

  return targetIndexes.find(
    (index) => getItemStart(index) > input.currentOffset + scrollThreshold,
  ) ?? null
}

export component MessageList<Item>(props: MessageListProps<Item>) {
  const rootRef = useRef<HTMLDivElement | null>(null)
  const spacerRef = useRef<HTMLDivElement | null>(null)
  const lastItemNavigationKeyRef = useRef<number | string | null>(null)
  const previousScrollHeightRef = useRef(0)
  const previousItemKeysRef = useRef<readonly string[]>([])
  const userScrollIntentRef = useRef(false)
  const [isStickingToBottom, setIsStickingToBottom] = useState(false)
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
    if (props.stickToBottomKey === undefined) {
      return
    }

    userScrollIntentRef.current = false
    setIsStickingToBottom(true)
  }, [props.stickToBottomKey])

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
  const totalSize = virtualizer.getTotalSize()

  const scrollToBottom = () => {
    const scrollElement = tabContext.scrollParentRef.current

    if (!scrollElement || props.items.length === 0) {
      return
    }

    virtualizer.scrollToIndex(props.items.length - 1, {
      align: "end",
    })
    scrollElement.scrollTop = scrollElement.scrollHeight
  }

  useLayoutEffect(() => {
    const scrollElement = tabContext.scrollParentRef.current

    if (!props.itemNavigation || !scrollElement || viewport.width <= 0) {
      return
    }

    if (lastItemNavigationKeyRef.current === props.itemNavigation.key) {
      return
    }

    lastItemNavigationKeyRef.current = props.itemNavigation.key

    const targetIndex = findNavigationTargetIndex({
      currentOffset: Math.max(0, scrollElement.scrollTop - viewport.scrollMargin),
      defaultRowHeight: props.defaultRowHeight ?? 1,
      direction: props.itemNavigation.direction,
      items: props.items,
      matchesItem: props.itemNavigation.matchesItem,
      measurements: virtualizer.measurementsCache,
    })

    if (targetIndex === null) {
      return
    }

    userScrollIntentRef.current = true
    setIsStickingToBottom(false)
    virtualizer.scrollToIndex(targetIndex, {
      align: "start",
    })
  }, [
    props.itemNavigation?.key,
    tabContext.scrollParentRef,
    viewport.scrollMargin,
    viewport.width,
    virtualizer,
  ])

  useLayoutEffect(() => {
    const scrollElement = tabContext.scrollParentRef.current

    if (!scrollElement) {
      return
    }

    const markUserScrollIntent = () => {
      userScrollIntentRef.current = true
    }
    const markScrollbarPointerIntent = (event: PointerEvent) => {
      if (event.target !== event.currentTarget) {
        return
      }

      userScrollIntentRef.current = true
    }
    const markKeyboardScrollIntent = (event: KeyboardEvent) => {
      if (event.target !== event.currentTarget) {
        return
      }

      if (
        event.key === "ArrowUp" || event.key === "PageUp" || event.key === "Home" ||
          event.key === " "
      ) {
        userScrollIntentRef.current = true
      }
    }
    const stopStickingAfterUserScroll = () => {
      if (!isStickingToBottom || !userScrollIntentRef.current) {
        return
      }

      if (isScrolledNearBottom(scrollElement)) {
        return
      }

      setIsStickingToBottom(false)
    }

    scrollElement.addEventListener("wheel", markUserScrollIntent)
    scrollElement.addEventListener("touchstart", markUserScrollIntent)
    scrollElement.addEventListener("pointerdown", markScrollbarPointerIntent)
    scrollElement.addEventListener("keydown", markKeyboardScrollIntent)
    scrollElement.addEventListener("scroll", stopStickingAfterUserScroll)

    return () => {
      scrollElement.removeEventListener("wheel", markUserScrollIntent)
      scrollElement.removeEventListener("touchstart", markUserScrollIntent)
      scrollElement.removeEventListener("pointerdown", markScrollbarPointerIntent)
      scrollElement.removeEventListener("keydown", markKeyboardScrollIntent)
      scrollElement.removeEventListener("scroll", stopStickingAfterUserScroll)
    }
  }, [isStickingToBottom, tabContext.scrollParentRef])

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
      !isStickingToBottom || props.items.length === 0 || viewport.width <= 0 ||
        !viewport.scrollMarginMeasured
    ) {
      return
    }

    scrollToBottom()
    requestAnimationFrame(scrollToBottom)
  }, [
    isStickingToBottom,
    props.items.length,
    totalSize,
    viewport.scrollMarginMeasured,
    viewport.width,
  ])

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
