import { cx } from "@goddard-ai/styled-system/css"

import type { MessageListRow } from "../message-list.tsx"
import { MarkdownMessage } from "../markdown-message.tsx"
import type { SessionTranscriptTextMessage } from "~/sessions/models.ts"
import { getBubbleMaxWidth } from "./layout.ts"
import { TranscriptMetaRow } from "./meta-row.tsx"
import {
  assistantBubbleClass,
  bubbleFrameClass,
  rowClass,
  rowColumnClass,
  rowInnerClass,
  systemBubbleClass,
  systemTextClass,
  userBubbleClass,
} from "./styles.ts"

/** Renders one text transcript row with Markdown-rich assistant and user bubbles. */
export function TextTranscriptRow(props: { row: MessageListRow<SessionTranscriptTextMessage> }) {
  const message = props.row.item
  const bubbleWidth = getBubbleMaxWidth(props.row.viewportWidth, message)
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
            {message.role === "system" ? (
              <div class={systemTextClass}>{message.text}</div>
            ) : (
              <MarkdownMessage
                markdown={message.text}
                role={message.role}
                streaming={message.streaming}
              />
            )}
          </div>
        </div>
      </div>
    </article>
  )
}
