import { cx } from "@goddard-ai/styled-system/css"
import { useVirtualizer, type VirtualItem } from "@tanstack/react-virtual"
import { AlertCircle, ChevronUp, LoaderCircle } from "lucide-react"
import { useLayoutEffect, useMemo, useRef, useState } from "preact/hooks"
import { useListener, type SigmaTarget } from "preact-sigma"

import type { SessionChatPageEvents } from "./page-events.ts"
import { estimateTranscriptRowHeight } from "./transcript/layout.ts"
import styles, {
  BODY_LINE_HEIGHT,
  DEFAULT_ROW_HEIGHT,
  ROW_GAP,
  TURN_STOP_ROW_HEIGHT,
  VIRTUAL_OVERSCAN_PX,
} from "./transcript/transcript.style.ts"
import { PermissionRequestTranscriptRow } from "./transcript/permission-request-row.tsrx"
import { PlanUpdateTranscriptRow } from "./transcript/plan-update-row.tsrx"
import { TextTranscriptRow } from "./transcript/text-row.tsrx"
import { ToolTranscriptRow } from "./transcript/tool-row.tsrx"
import { TurnStopTranscriptRow } from "./transcript/turn-stop-row.tsrx"
import { WorkDrawerTranscriptRow } from "./transcript/work-drawer-row.tsrx"
import type {
  SessionTranscriptItem,
  SessionTranscriptPermissionRequest,
  SessionTranscriptPlanUpdate,
  SessionTranscriptTextMessage,
  SessionTranscriptToolCall,
  SessionTranscriptTurnStop,
  SessionTranscriptWorkDrawer,
} from "~/sessions/models.ts"
import { useTabContext } from "~/tab-context.tsrx"

/** One transcript row rendered by the session chat surface. */
export type TranscriptMessage = SessionTranscriptItem

/** Props accepted by the dumb session transcript component. */
export type TranscriptProps = {
  messages: readonly TranscriptMessage[];
  class?: string;
  initialScrollPosition?: "top" | "bottom";
  pageEvents?: SigmaTarget<SessionChatPageEvents>;
  onSelectPermissionOption?: (input: {
    optionId: string;
    requestId: SessionTranscriptPermissionRequest["requestId"];
  }) => Promise<void> | void;
  olderHistory?: {
    canLoad: boolean;
    error: string | null;
    loading: boolean;
    onLoad: () => Promise<void> | void;
  };
  showThinkingLabel?: boolean;
  stickToBottomKey?: number | string;
}

const PROMPT_NAVIGATION_TOP_GAP = 8

type CoalescedToolTranscriptRow = {
  kind: "toolGroup";
  id: string;
  toolCalls: readonly SessionTranscriptToolCall[];
}

type RenderTranscriptItem =
  | SessionTranscriptTextMessage
  | CoalescedToolTranscriptRow
  | SessionTranscriptTurnStop
  | SessionTranscriptPermissionRequest
  | SessionTranscriptPlanUpdate
  | SessionTranscriptWorkDrawer

type TranscriptRowContext = {
  item: RenderTranscriptItem;
  index: number;
  viewportWidth: number;
}

type TranscriptViewport = {
  scrollMarginMeasured: boolean;
  scrollMargin: number;
  width: number;
}

function coalesceTranscriptItems(messages: readonly TranscriptMessage[]) {
  const renderItems: RenderTranscriptItem[] = []
  let pendingToolCalls: SessionTranscriptToolCall[] = []

  const flushToolCalls = () => {
    if (pendingToolCalls.length === 0) {
      return
    }

    renderItems.push({
      kind: "toolGroup",
      id: `${pendingToolCalls[0]!.id}:group:${pendingToolCalls[pendingToolCalls.length - 1]!.id}`,
      toolCalls: pendingToolCalls,
    })
    pendingToolCalls = []
  }

  for (const message of messages) {
    if (message.kind === "toolCall") {
      pendingToolCalls.push(message)
      continue
    }

    flushToolCalls()

    if (message.kind === "turnStop") {
      renderItems.push(message)
      continue
    }

    if (message.kind === "permissionRequest") {
      renderItems.push(message)
      continue
    }

    if (message.kind === "planUpdate") {
      renderItems.push(message)
      continue
    }

    if (message.kind === "workDrawer") {
      renderItems.push(message)
      continue
    }

    if (message.role === "system") {
      continue
    }

    renderItems.push(message)
  }

  flushToolCalls()

  return renderItems
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

function getPromptNavigationTopInset(scrollElement: HTMLDivElement) {
  const headerElement = scrollElement.querySelector<HTMLElement>("[data-session-chat-header]")

  return (headerElement?.getBoundingClientRect().height ?? 0) + PROMPT_NAVIGATION_TOP_GAP
}

function getPromptNavigationTargetIndexes(items: readonly RenderTranscriptItem[]) {
  return items.flatMap(
    (item, index) => item.kind === "message" && item.role === "user" ? [index] : [],
  )
}

function findPromptNavigationTargetIndex(input: {
  currentItemId: string | null;
  currentOffset: number;
  currentVisibleEndOffset: number;
  defaultRowHeight: number;
  direction: "next" | "previous";
  items: readonly RenderTranscriptItem[];
  measurements: readonly VirtualItem[];
}) {
  const targetIndexes = getPromptNavigationTargetIndexes(input.items)

  if (targetIndexes.length === 0) {
    return null
  }

  if (input.currentItemId !== null) {
    const currentTargetIndex = targetIndexes.findIndex(
      (index) => input.items[index]?.id === input.currentItemId,
    )

    if (currentTargetIndex >= 0) {
      return input.direction === "previous"
        ? targetIndexes[currentTargetIndex - 1] ?? null
        : targetIndexes[currentTargetIndex + 1] ?? null
    }
  }

  const getItemStart = (index: number) => input.measurements[index]?.start ??
    index * input.defaultRowHeight
  const scrollThreshold = 8

  if (input.direction === "next") {
    const currentVisibleTargetIndex = targetIndexes.findIndex((index) => {
      const itemStart = getItemStart(index)

      return itemStart >= input.currentOffset - scrollThreshold &&
        itemStart < input.currentVisibleEndOffset - scrollThreshold
    })

    if (currentVisibleTargetIndex >= 0) {
      return targetIndexes[currentVisibleTargetIndex + 1] ?? null
    }
  }

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

function estimateToolGroupRowHeight(
  toolCalls: readonly SessionTranscriptToolCall[],
  viewportWidth: number,
) {
  const summaryWidth = Math.max(240, viewportWidth - 48)
  const approximateCharactersPerLine = Math.max(18, Math.floor(summaryWidth / 8))
  const joinedTitles = toolCalls.map((toolCall) => toolCall.title).join(", ")
  const lineCount = Math.max(1, Math.ceil(joinedTitles.length / approximateCharactersPerLine))

  return lineCount * BODY_LINE_HEIGHT + ROW_GAP
}

function estimateRenderItemSize(item: RenderTranscriptItem | undefined, viewportWidth: number) {
  if (!item) {
    return DEFAULT_ROW_HEIGHT
  }

  return Math.max(
    1,
    Math.round(
      item.kind === "toolGroup"
        ? estimateToolGroupRowHeight(item.toolCalls, viewportWidth)
        : item.kind === "workDrawer"
          ? estimateTranscriptRowHeight(item, viewportWidth)
          : item.kind === "turnStop"
            ? TURN_STOP_ROW_HEIGHT
            : estimateTranscriptRowHeight(item, viewportWidth),
    ),
  )
}

function getEstimatedBottomOffset(input: {
  items: readonly RenderTranscriptItem[];
  scrollElement: HTMLDivElement | null;
  viewport: TranscriptViewport;
}) {
  if (!input.scrollElement || input.items.length === 0 || input.viewport.width <= 0) {
    return 0
  }

  const estimatedHeight = input.items.reduce(
    (height, item) => height + estimateRenderItemSize(item, input.viewport.width),
    0,
  )

  return Math.max(
    0,
    input.viewport.scrollMargin + estimatedHeight - input.scrollElement.clientHeight,
  )
}

/** Renders one transcript row using the row-specific presentation path. */
function TranscriptRow(props: {
  row: TranscriptRowContext;
  onSelectPermissionOption?: TranscriptProps["onSelectPermissionOption"];
}) {
  const item = props.row.item
  switch (item.kind) {
    case "toolGroup":
      return <ToolTranscriptRow groupId={item.id} toolCalls={item.toolCalls} />
    case "permissionRequest":
      return <PermissionRequestTranscriptRow
        message={item}
        onSelectOption={props.onSelectPermissionOption}
      />
    case "planUpdate":
      return <PlanUpdateTranscriptRow message={item} />
    case "workDrawer":
      return <WorkDrawerTranscriptRow message={item} />
    case "turnStop":
      return <TurnStopTranscriptRow
        row={props.row as
          TranscriptRowContext & {
            item: SessionTranscriptTurnStop;
          }}
      />
    case "message":
      return <TextTranscriptRow
        row={props.row as
          TranscriptRowContext & {
            item: SessionTranscriptTextMessage;
          }}
      />
    default:
      throw new Error("Unsupported transcript row.")
  }
}

function OlderHistoryPager(props: {
  olderHistory: NonNullable<TranscriptProps["olderHistory"]>;
}) @{
  if (!props.olderHistory.canLoad && !props.olderHistory.loading && !props.olderHistory.error) {
    return null
  }

  <div class={styles.historyPager}>
    @if (props.olderHistory.canLoad || props.olderHistory.loading) {
      <button
        class={styles.historyLoadButton}
        disabled={props.olderHistory.loading}
        type="button"
        onClick={() => {
          void props.olderHistory.onLoad()
        }}
      >
        @if (props.olderHistory.loading) {
          <>
            <LoaderCircle class={styles.historyLoadButtonIcon} size={14} strokeWidth={2.2} />
            Loading older messages
          </>
        } @else {
          <>
            <ChevronUp size={14} strokeWidth={2.2} />
            Load older messages
          </>
        }
      </button>
    }
    @if (props.olderHistory.error) {
      <div class={styles.historyLoadError} role="alert">
        <AlertCircle size={14} strokeWidth={2.2} />
        <span>{props.olderHistory.error}</span>
      </div>
    }
  </div>
}

function ThinkingLabel() @{
  <div class={styles.thinkingLabelRow} aria-live="polite">
    <span class={styles.thinkingLabel}>Thinking...</span>
  </div>
}

/** Renders one chat transcript with virtualized row layout. */
export function Transcript(props: TranscriptProps) @{
  const effectiveInitialScroll = props.initialScrollPosition ?? "bottom"
  const renderItems = useMemo(() => coalesceTranscriptItems(props.messages), [
    props.messages,
  ])
  const rootRef = useRef<HTMLDivElement | null>(null)
  const spacerRef = useRef<HTMLDivElement | null>(null)
  const lastPromptNavigationItemIdRef = useRef<string | null>(null)
  const previousScrollHeightRef = useRef(0)
  const previousItemKeysRef = useRef<readonly string[]>([])
  const userScrollIntentRef = useRef(false)
  const initialBottomScrollStartedRef = useRef(false)
  const [isStickingToBottom, setIsStickingToBottom] = useState(false)
  const tabContext = useTabContext()
  const [viewport, setViewport] = useState<TranscriptViewport>({
    scrollMarginMeasured: false,
    scrollMargin: 0,
    width: 0,
  })

  const itemKeys = useMemo(() => renderItems.map((item) => item.id), [renderItems])

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
    count: renderItems.length,
    enabled: viewport.width > 0,
    estimateSize: (index) => estimateRenderItemSize(renderItems[index], viewport.width),
    getItemKey: (index) => itemKeys[index] ?? index,
    getScrollElement: () => tabContext.scrollParentRef.current,
    initialOffset: effectiveInitialScroll === "bottom"
      ? () => getEstimatedBottomOffset({
          items: renderItems,
          scrollElement: tabContext.scrollParentRef.current,
          viewport,
        })
      : undefined,
    overscan: Math.max(1, Math.ceil(VIRTUAL_OVERSCAN_PX / DEFAULT_ROW_HEIGHT)),
    scrollMargin: viewport.scrollMargin,
    useFlushSync: false,
  })
  const totalSize = virtualizer.getTotalSize()

  const scrollToBottom = () => {
    const scrollElement = tabContext.scrollParentRef.current

    if (!scrollElement || renderItems.length === 0) {
      return
    }

    virtualizer.scrollToIndex(renderItems.length - 1, {
      align: "end",
    })
    scrollElement.scrollTop = scrollElement.scrollHeight
  }

  useListener(props.pageEvents ?? null, "scrollToPrompt", ({ direction }) => {
    const scrollElement = tabContext.scrollParentRef.current

    if (!scrollElement || viewport.width <= 0) {
      return
    }

    const topInset = getPromptNavigationTopInset(scrollElement)
    const targetIndex = findPromptNavigationTargetIndex({
      currentItemId: lastPromptNavigationItemIdRef.current,
      currentOffset: Math.max(0, scrollElement.scrollTop - viewport.scrollMargin + topInset),
      currentVisibleEndOffset: Math.max(
        0,
        scrollElement.scrollTop - viewport.scrollMargin + scrollElement.clientHeight,
      ),
      defaultRowHeight: DEFAULT_ROW_HEIGHT,
      direction,
      items: renderItems,
      measurements: virtualizer.measurementsCache,
    })

    if (targetIndex === null) {
      return
    }

    lastPromptNavigationItemIdRef.current = renderItems[targetIndex]?.id ?? null
    userScrollIntentRef.current = true
    setIsStickingToBottom(false)
    virtualizer.scrollToIndex(targetIndex, {
      align: "start",
    })
    requestAnimationFrame(() => {
      scrollElement.scrollTop = Math.max(0, scrollElement.scrollTop - topInset)
    })
  })

  useLayoutEffect(() => {
    const scrollElement = tabContext.scrollParentRef.current

    if (!scrollElement) {
      return
    }

    const markUserScrollIntent = () => {
      lastPromptNavigationItemIdRef.current = null
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

    if (props.olderHistory) {
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
  }, [itemKeys, props.olderHistory, tabContext.scrollParentRef])

  useLayoutEffect(() => {
    if (
      !isStickingToBottom || renderItems.length === 0 || viewport.width <= 0 ||
        !viewport.scrollMarginMeasured
    ) {
      return
    }

    scrollToBottom()
    requestAnimationFrame(scrollToBottom)
  }, [
    isStickingToBottom,
    renderItems.length,
    totalSize,
    viewport.scrollMarginMeasured,
    viewport.width,
  ])

  useLayoutEffect(() => {
    if (
      effectiveInitialScroll !== "bottom" || renderItems.length === 0 || viewport.width <= 0 ||
        !viewport.scrollMarginMeasured
    ) {
      return
    }

    if (initialBottomScrollStartedRef.current) {
      return
    }

    initialBottomScrollStartedRef.current = true
    userScrollIntentRef.current = false
    setIsStickingToBottom(true)
  }, [
    effectiveInitialScroll,
    renderItems.length,
    totalSize,
    viewport.scrollMarginMeasured,
    viewport.width,
  ])

  <div class={cx(styles.transcriptViewport, props.class)}>
    <div ref={rootRef} style={{ minHeight: "100%" }}>
      @if (viewport.width <= 0) {
        <div class={styles.loadingState}>Preparing transcript layout...</div>
      } @else {
        <>
          {props.olderHistory ? <OlderHistoryPager olderHistory={props.olderHistory} /> : null}

          <div
            ref={spacerRef}
            class={styles.virtualSpacer}
            style={{
              height: `${virtualizer.getTotalSize()}px`,
            }}
          >{virtualizer.getVirtualItems().map((virtualItem) => {
            const item = renderItems[virtualItem.index]

            if (!item) {
              return null
            }

            return <div
              key={virtualItem.key}
              data-index={virtualItem.index}
              ref={virtualizer.measureElement}
              class={styles.virtualRow}
              style={{
                transform: `translateY(${virtualItem.start - viewport.scrollMargin}px)`,
              }}
            >
              <TranscriptRow
                onSelectPermissionOption={props.onSelectPermissionOption}
                row={{
                  item,
                  index: virtualItem.index,
                  viewportWidth: viewport.width,
                }}
              />
            </div>
          })}</div>
        </>
      }
    </div>

    @if (props.showThinkingLabel) {
      <ThinkingLabel />
    }
  </div>
}
