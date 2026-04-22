import { cx } from "@goddard-ai/styled-system/css"
import { useMemo } from "preact/hooks"

import type {
  SessionTranscriptItem,
  SessionTranscriptTextMessage,
  SessionTranscriptToolCall,
} from "~/sessions/models.ts"
import { MessageList, type MessageListRow } from "./message-list.tsrx"
import { estimateTranscriptRowHeight } from "./transcript/layout.ts"
import {
  BODY_LINE_HEIGHT,
  DEFAULT_ROW_HEIGHT,
  loadingStateClass,
  ROW_GAP,
  transcriptViewportClass,
  VIRTUAL_OVERSCAN_PX,
} from "./transcript/styles.ts"
import { TextTranscriptRow } from "./transcript/text-row.tsrx"
import { ToolTranscriptRow } from "./transcript/tool-row.tsrx"

/** One transcript row rendered by the session chat surface. */
export type TranscriptMessage = SessionTranscriptItem

/** Props accepted by the dumb session transcript component. */
export type TranscriptProps = {
  messages: readonly TranscriptMessage[];
  class?: string;
  initialScrollPosition?: "top" | "bottom";
  scrollCacheKey?: string;
}

type CoalescedToolTranscriptRow = {
  kind: "toolGroup";
  id: string;
  toolCalls: readonly SessionTranscriptToolCall[];
}

type RenderTranscriptItem = SessionTranscriptTextMessage | CoalescedToolTranscriptRow

function coalesceTranscriptItems(messages: readonly TranscriptMessage[]) {
  const renderItems: RenderTranscriptItem[] = []
  let pendingToolCalls: SessionTranscriptToolCall[] = []

  const flushToolCalls = () => {
    if (pendingToolCalls.length === 0) {
      return
    }

    renderItems.push(
      {
        kind: "toolGroup",
        id: `${pendingToolCalls[0]!.id}:group:${pendingToolCalls[pendingToolCalls.length - 1]!.id}`,
        toolCalls: pendingToolCalls,
      },
    )
    pendingToolCalls = []
  }

  for (const message of messages) {
    if (message.kind === "toolCall") {
      pendingToolCalls.push(message)
      continue
    }

    flushToolCalls()

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
function TranscriptRow(props: { row: MessageListRow<RenderTranscriptItem> }) {
  if (props.row.item.kind === "toolGroup") {
    return <tsx:react>
      <ToolTranscriptRow groupId={props.row.item.id} toolCalls={props.row.item.toolCalls} />
    </tsx:react>
  }

  return <tsx:react>
    <TextTranscriptRow row={props.row as MessageListRow<SessionTranscriptTextMessage>} />
  </tsx:react>
}

/** Renders one chat transcript inside the shared message-list virtualizer. */
export function Transcript(props: TranscriptProps) {
  const effectiveInitialScroll = props.initialScrollPosition ?? "bottom"
  const renderItems = useMemo(() => coalesceTranscriptItems(props.messages), [
    props.messages,
  ])

  return <tsx:react>
    <div class={cx(transcriptViewportClass, props.class)}>
      <MessageList defaultRowHeight={DEFAULT_ROW_HEIGHT} estimateRowHeight={(
        message,
        _index,
        viewportWidth,
      ) => message.kind === "toolGroup"
        ? estimateToolGroupRowHeight(message.toolCalls, viewportWidth)
        : estimateTranscriptRowHeight(message, viewportWidth)} getItemKey={(
        message,
      ) => message.id} initialScrollPosition={effectiveInitialScroll} items={renderItems} loadingFallback={<tsx:react>
        <div class={loadingStateClass}>Preparing transcript layout...</div>
      </tsx:react>} overscanPx={VIRTUAL_OVERSCAN_PX} renderRow={(row) => <tsx:react>
        <TranscriptRow row={row} />
      </tsx:react>} scrollCacheKey={props.scrollCacheKey} />
    </div>
  </tsx:react>
}
