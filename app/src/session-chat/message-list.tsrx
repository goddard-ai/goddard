import { useEffect, useMemo, useRef } from "preact/hooks"
import { Virtuoso, type StateSnapshot, type VirtuosoHandle } from "react-virtuoso"

import { useTabViewport } from "~/tab-viewport.tsrx"
import styles from "./message-list.style.ts"

const messageListStateCache = new Map<string, StateSnapshot>()

/** One visible message row handed to the row renderer. */
export type MessageListRow<Item> = {
  item: Item;
  index: number;
  viewportWidth: number;
}

/** Props accepted by the session-chat-owned Virtuoso wrapper. */
export type MessageListProps<Item> = {
  items: readonly Item[];
  overscanPx?: number;
  initialScrollPosition?: "top" | "bottom";
  scrollCacheKey?: string;
  loadingFallback?: preact.ComponentChild;
  defaultRowHeight?: number;
  getItemKey?: (item: Item, index: number) => string;
  estimateRowHeight?: (item: Item, index: number, viewportWidth: number) => number;
  renderRow: (row: MessageListRow<Item>) => preact.ComponentChild;
}

/** Returns the first mount position for a list that should open from the top or bottom. */
function getInitialTopMostItemIndex<Item>(
  items: readonly Item[],
  initialScrollPosition: "top" | "bottom",
  restoreState: StateSnapshot | undefined,
) {
  if (restoreState || items.length === 0) {
    return undefined
  }

  if (initialScrollPosition === "bottom") {
    return {
      index: "LAST" as const,
      align: "end" as const,
    }
  }

  return 0
}

/** Renders the transcript's Virtuoso message list inside the nearest tab viewport provider. */
export component MessageList<Item>(props: MessageListProps<Item>) {
  const viewport = useTabViewport()
  const virtuosoRef = useRef<VirtuosoHandle | null>(null)
  const scrollCacheKey = props.scrollCacheKey
  const effectiveInitialScroll = props.initialScrollPosition ?? "top"
  const overscanPx = props.overscanPx ?? 320
  const restoreState =
    scrollCacheKey ? messageListStateCache.get(scrollCacheKey) : undefined
  const initialTopMostItemIndex = getInitialTopMostItemIndex(
    props.items,
    effectiveInitialScroll,
    restoreState,
  )

  useEffect(() => {
    return () => {
      if (!scrollCacheKey) {
        return
      }

      virtuosoRef.current?.getState((state) => {
        messageListStateCache.set(scrollCacheKey, state)
      })
    }
  }, [scrollCacheKey])

  const heightEstimates = useMemo(() => {
    const estimateRowHeight = props.estimateRowHeight

    if (!estimateRowHeight || viewport.width <= 0) {
      return undefined
    }

    return props.items.map(
      (item, index) => Math.max(1, Math.round(estimateRowHeight(item, index, viewport.width))),
    )
  }, [props.estimateRowHeight, props.items, viewport.width])

  if (!viewport.fontsReady || viewport.width <= 0 || !viewport.viewportRef.current) {
    <tsx:react>
      {props.loadingFallback ?? <div class={styles.loading}>Preparing paragraph layout...</div>}
    </tsx:react>
    return
  }

  <tsx:react>
    <Virtuoso ref={virtuosoRef} alignToBottom={effectiveInitialScroll ===
      "bottom"} computeItemKey={(index, item) => props.getItemKey?.(item, index) ??
      index} customScrollParent={viewport.viewportRef.current} data={props.items} defaultItemHeight={props.defaultRowHeight} heightEstimates={heightEstimates} itemContent={(
      index,
      item,
    ) => props.renderRow(
      {
        item,
        index,
        viewportWidth: viewport.width,
      },
    )} overscan={{
      main: overscanPx,
      reverse: overscanPx,
    }} style={{
      minHeight: "100%",
    }} />
  </tsx:react>
}
