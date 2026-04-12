import { cx } from "@goddard-ai/styled-system/css"

import type {
  SessionTranscriptItem,
  SessionTranscriptTextMessage,
  SessionTranscriptToolCall,
} from "~/sessions/models.ts"
import { MessageList, type MessageListRow } from "./message-list.tsx"
import { estimateTranscriptRowHeight } from "./transcript/layout.ts"
import {
  DEFAULT_ROW_HEIGHT,
  loadingStateClass,
  transcriptViewportClass,
  VIRTUAL_OVERSCAN_PX,
} from "./transcript/styles.ts"
import { TextTranscriptRow } from "./transcript/text-row.tsx"
import { ToolTranscriptRow } from "./transcript/tool-row.tsx"

/** One transcript row rendered by the session chat surface. */
export type TranscriptMessage = SessionTranscriptItem

/** Props accepted by the dumb session transcript component. */
export type TranscriptProps = {
  messages: readonly TranscriptMessage[]
  class?: string
  initialScrollPosition?: "top" | "bottom"
  scrollCacheKey?: string
}

/** Renders one transcript row using the row-specific presentation path. */
function TranscriptRow(props: { row: MessageListRow<TranscriptMessage> }) {
  if (props.row.item.kind === "toolCall") {
    return <ToolTranscriptRow row={props.row as MessageListRow<SessionTranscriptToolCall>} />
  }

  return <TextTranscriptRow row={props.row as MessageListRow<SessionTranscriptTextMessage>} />
}

/** Renders one chat transcript inside the shared message-list virtualizer. */
export function Transcript(props: TranscriptProps) {
  const effectiveInitialScroll = props.initialScrollPosition ?? "bottom"

  return (
    <div class={cx(transcriptViewportClass, props.class)}>
      <MessageList<TranscriptMessage>
        defaultRowHeight={DEFAULT_ROW_HEIGHT}
        estimateRowHeight={(message, _index, viewportWidth) =>
          estimateTranscriptRowHeight(message, viewportWidth)
        }
        getItemKey={(message) => message.id}
        initialScrollPosition={effectiveInitialScroll}
        items={props.messages}
        loadingFallback={<div class={loadingStateClass}>Preparing transcript layout...</div>}
        overscanPx={VIRTUAL_OVERSCAN_PX}
        renderRow={(row) => <TranscriptRow row={row} />}
        scrollCacheKey={props.scrollCacheKey}
      />
    </div>
  )
}
