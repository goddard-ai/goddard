import { cx } from "@goddard-ai/styled-system/css"
import { AlertCircle, ChevronUp, LoaderCircle } from "lucide-react"
import { useMemo } from "preact/hooks"

import type {
  SessionTranscriptItem,
  SessionTranscriptPermissionRequest,
  SessionTranscriptPlanUpdate,
  SessionTranscriptTextMessage,
  SessionTranscriptToolCall,
  SessionTranscriptTurnStop,
} from "~/sessions/models.ts"
import { MessageList, type MessageListRow } from "./message-list.tsrx"
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

/** One transcript row rendered by the session chat surface. */
export type TranscriptMessage = SessionTranscriptItem

/** Props accepted by the dumb session transcript component. */
export type TranscriptProps = {
  messages: readonly TranscriptMessage[];
  class?: string;
  initialScrollPosition?: "top" | "bottom";
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
  scrollCacheKey?: string;
  showThinkingLabel?: boolean;
}

type CoalescedToolTranscriptRow = {
  kind: "toolGroup";
  id: string;
  toolCalls: readonly SessionTranscriptToolCall[];
}

type RenderTranscriptItem =
  SessionTranscriptTextMessage | CoalescedToolTranscriptRow | SessionTranscriptTurnStop | SessionTranscriptPermissionRequest | SessionTranscriptPlanUpdate

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

    if (message.role === "system") {
      continue
    }

    renderItems.push(message)
  }

  flushToolCalls()

  return renderItems
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

/** Renders one transcript row using the row-specific presentation path. */
component TranscriptRow(props: {
  row: MessageListRow<RenderTranscriptItem>;
  onSelectPermissionOption?: TranscriptProps["onSelectPermissionOption"];
}) {
  const item = props.row.item
  switch (item.kind) {
    case "toolGroup":
      <ToolTranscriptRow groupId={item.id} toolCalls={item.toolCalls} />
      return
    case "permissionRequest":
      <PermissionRequestTranscriptRow
        message={item}
        onSelectOption={props.onSelectPermissionOption}
      />
      return
    case "planUpdate":
      <PlanUpdateTranscriptRow message={item} />
      return
    case "turnStop":
      <TurnStopTranscriptRow row={props.row as MessageListRow<SessionTranscriptTurnStop>} />
      return
    case "message":
      <TextTranscriptRow row={props.row as MessageListRow<SessionTranscriptTextMessage>} />
      return
    default:
      throw new Error("Unsupported transcript row.")
  }
}

component OlderHistoryPager(props: {
  olderHistory: NonNullable<TranscriptProps["olderHistory"]>;
}) {
  if (!props.olderHistory.canLoad && !props.olderHistory.loading && !props.olderHistory.error) {
    return
  }

  <div class={styles.historyPager}>
    if (props.olderHistory.canLoad || props.olderHistory.loading) {
      <button
        class={styles.historyLoadButton}
        disabled={props.olderHistory.loading}
        type="button"
        onClick={() => {
          void props.olderHistory.onLoad()
        }}
      >
        if (props.olderHistory.loading) {
          <LoaderCircle class={styles.historyLoadButtonIcon} size={14} strokeWidth={2.2} />
          "Loading older messages"
        } else {
          <ChevronUp size={14} strokeWidth={2.2} />
          "Load older messages"
        }
      </button>
    }
    if (props.olderHistory.error) {
      <div class={styles.historyLoadError} role="alert">
        <AlertCircle size={14} strokeWidth={2.2} />
        <span>{props.olderHistory.error}</span>
      </div>
    }
  </div>
}

component ThinkingLabel() {
  <div class={styles.thinkingLabelRow} aria-live="polite">
    <span class={styles.thinkingLabel}>"Thinking..."</span>
  </div>
}

/** Renders one chat transcript inside the shared message-list virtualizer. */
export component Transcript(props: TranscriptProps) {
  const effectiveInitialScroll = props.initialScrollPosition ?? "bottom"
  const renderItems = useMemo(() => coalesceTranscriptItems(props.messages), [
    props.messages,
  ])

  <div class={cx(styles.transcriptViewport, props.class)}>
    <MessageList
      defaultRowHeight={DEFAULT_ROW_HEIGHT}
      estimateRowHeight={(message, _index, viewportWidth) => message.kind === "toolGroup"
        ? estimateToolGroupRowHeight(message.toolCalls, viewportWidth)
        : message.kind === "turnStop"
          ? TURN_STOP_ROW_HEIGHT
          : estimateTranscriptRowHeight(message, viewportWidth)}
      getItemKey={(message) => message.id}
      header={<tsrx>
        if (props.olderHistory) {
          <OlderHistoryPager olderHistory={props.olderHistory} />
        }
      </tsrx>}
      initialScrollPosition={effectiveInitialScroll}
      items={renderItems}
      loadingFallback={<tsrx>
        <div class={styles.loadingState}>"Preparing transcript layout..."</div>
      </tsrx>}
      overscanPx={VIRTUAL_OVERSCAN_PX}
      preserveScrollOnPrepend={Boolean(props.olderHistory)}
      renderRow={(row) => (
        <tsrx>
          <TranscriptRow {row} onSelectPermissionOption={props.onSelectPermissionOption} />
        </tsrx>
      )}
      scrollCacheKey={props.scrollCacheKey}
    />
    if (props.showThinkingLabel) {
      <ThinkingLabel />
    }
  </div>
}
