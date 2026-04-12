import { cx } from "@goddard-ai/styled-system/css"
import { useMemo } from "preact/hooks"
import type { MessageListRow } from "../message-list.tsx"
import type { SessionTranscriptTextMessage } from "~/sessions/models.ts"
import { getTranscriptBubbleWidth, getTranscriptTextWidth, measureParagraph } from "./layout.ts"
import { TranscriptMetaRow } from "./meta-row.tsx"
import {
  assistantBubbleClass,
  assistantLineClass,
  bubbleFrameClass,
  rowClass,
  rowColumnClass,
  rowInnerClass,
  systemBubbleClass,
  systemLineClass,
  transcriptLineClass,
  userBubbleClass,
  userLineClass,
} from "./styles.ts"

/** Renders one text transcript row with manual line rendering that matches Pretext layout. */
export function TextTranscriptRow(props: { row: MessageListRow<SessionTranscriptTextMessage> }) {
  const message = props.row.item
  const paragraphMaxWidth = getTranscriptTextWidth(message, props.row.viewportWidth)
  const paragraph = useMemo(
    () => measureParagraph(message.text, paragraphMaxWidth),
    [message.text, paragraphMaxWidth],
  )
  const bubbleWidth = getTranscriptBubbleWidth(message, props.row.viewportWidth, paragraph)
  const alignmentStyle =
    message.role === "user"
      ? { justifyContent: "flex-end" }
      : message.role === "system"
        ? { justifyContent: "center" }
        : { justifyContent: "flex-start" }

  const bubbleClass =
    message.role === "user"
      ? userBubbleClass
      : message.role === "system"
        ? systemBubbleClass
        : assistantBubbleClass

  const lineClass =
    message.role === "user"
      ? userLineClass
      : message.role === "system"
        ? systemLineClass
        : assistantLineClass

  return (
    <article class={rowClass}>
      <div class={rowInnerClass} style={alignmentStyle}>
        <div class={rowColumnClass} style={{ width: `${bubbleWidth}px` }}>
          <TranscriptMetaRow
            authorName={message.authorName}
            timestampLabel={message.timestampLabel}
            alignmentStyle={alignmentStyle}
          />
          <div class={cx(bubbleFrameClass, bubbleClass)}>
            {paragraph.lines.map((line, index) => (
              <div key={`${message.id}:${index}`} class={cx(transcriptLineClass, lineClass)}>
                {line.text.length > 0 ? line.text : "\u00a0"}
              </div>
            ))}
          </div>
        </div>
      </div>
    </article>
  )
}
