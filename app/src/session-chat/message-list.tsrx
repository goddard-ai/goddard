import { useEffect, useMemo, useRef } from "preact/hooks"
import { Virtuoso, type StateSnapshot, type VirtuosoHandle } from "react-virtuoso"

import { useTabViewport } from "~/tab-viewport.tsrx"
import styles from "./message-list.style.ts"

const messageListStateCache = new Map<string, StateSnapshot>()
const prependAnchorFirstItemIndex = 1000000

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
  header?: preact.ComponentChild;
  scrollCacheKey?: string;
  preserveScrollOnPrepend?: boolean;
  loadingFallback?: preact.ComponentChild;
  defaultRowHeight?: number;
  getItemKey?: (item: Item, index: number) => string;
  estimateRowHeight?: (item: Item, index: number, viewportWidth: number) => number;
  renderRow: (row: MessageListRow<Item>) => preact.ComponentChild;
}

type MessageListContext = {
  header: preact.ComponentChild;
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

component MessageListHeader(props: { context: MessageListContext }) {
  <>{props.context.header}</>
}

const messageListComponents = {
  Header: MessageListHeader,
}

/** Renders the transcript's Virtuoso message list inside the nearest tab viewport provider. */
export component MessageList<Item>(props: MessageListProps<Item>) {
  const virtuosoHandleRef = useRef<VirtuosoHandle | null>(null)
  const firstItemIndexRef = useRef(prependAnchorFirstItemIndex)
  const previousItemKeysRef = useRef<readonly string[]>([])

  const viewport = useTabViewport()
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
  const itemKeys = useMemo(() => buildItemKeys(props.items, props.getItemKey), [
    props.getItemKey,
    props.items,
  ])
  const headerContext = useMemo(
    () => ({
      header: props.header,
    }),
    [props.header],
  )

  if (props.preserveScrollOnPrepend) {
    const prependedItemCount = resolvePrependedItemCount(previousItemKeysRef.current, itemKeys)

    firstItemIndexRef.current = Math.max(1, firstItemIndexRef.current - prependedItemCount)
    previousItemKeysRef.current = itemKeys
  } else {
    firstItemIndexRef.current = prependAnchorFirstItemIndex
    previousItemKeysRef.current = itemKeys
  }
  const effectiveFirstItemIndex = props.preserveScrollOnPrepend ? firstItemIndexRef.current : 0

  useEffect(() => {
    return () => {
      if (!scrollCacheKey) {
        return
      }

      virtuosoHandleRef.current?.getState((state) => {
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
    {props.loadingFallback ??
      <>
        <div class={styles.loading}>
          {"Preparing paragraph layout..."}
        </div>
      </>}
    return
  }

  <Virtuoso
    {ref virtuosoHandleRef}
    alignToBottom={effectiveInitialScroll === "bottom"}
    components={props.header ? messageListComponents : undefined}
    computeItemKey={(index, item) => props.getItemKey?.(item, index - effectiveFirstItemIndex) ??
      index}
    context={headerContext}
    customScrollParent={viewport.viewportRef.current}
    data={props.items}
    defaultItemHeight={props.defaultRowHeight}
    firstItemIndex={props.preserveScrollOnPrepend ? effectiveFirstItemIndex : undefined}
    {heightEstimates}
    itemContent={(index, item) => props.renderRow(
      {
        item,
        index: index - effectiveFirstItemIndex,
        viewportWidth: viewport.width,
      },
    )}
    overscan={{
      main: overscanPx,
      reverse: overscanPx,
    }}
    style={{
      minHeight: "100%",
    }}
  />
}
