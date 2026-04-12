import { cx } from "@goddard-ai/styled-system/css"
import { BookOpen, Link2 } from "lucide-react"

import type {
  SessionTranscriptContentBlock,
  SessionTranscriptTextMessage,
} from "~/sessions/models.ts"
import { MarkdownMessage } from "../markdown-message.tsx"
import type { MessageListRow } from "../message-list.tsx"
import { getBubbleMaxWidth } from "./layout.ts"
import { TranscriptMetaRow } from "./meta-row.tsx"
import {
  assistantBubbleClass,
  attachmentCardClass,
  attachmentDetailClass,
  attachmentHeadingClass,
  attachmentIconClass,
  bubbleFrameClass,
  rowClass,
  rowColumnClass,
  rowInnerClass,
  systemBubbleClass,
  systemTextClass,
  transcriptContentClass,
  userBubbleClass,
} from "./styles.ts"

function isSkillResourceLink(
  block: Extract<SessionTranscriptContentBlock, { type: "resource_link" }>,
) {
  try {
    return new URL(block.uri).pathname.endsWith("/SKILL.md")
  } catch {
    return block.title?.toLowerCase().endsWith(" skill") ?? false
  }
}

function getResourceLinkDetail(
  block: Extract<SessionTranscriptContentBlock, { type: "resource_link" }>,
) {
  if (block.description) {
    return block.description
  }

  if (block.title && block.title !== block.name) {
    return block.title
  }

  return block.uri
}

function renderContentBlock(props: {
  block: SessionTranscriptContentBlock
  blockIndex: number
  lastTextBlockIndex: number
  message: SessionTranscriptTextMessage
}) {
  if (props.block.type === "resource_link") {
    return (
      <div key={`${props.message.id}:block:${props.blockIndex}`} class={attachmentCardClass}>
        <div class={attachmentHeadingClass}>
          <span class={attachmentIconClass} aria-hidden="true">
            {isSkillResourceLink(props.block) ? (
              <BookOpen size={14} strokeWidth={2.2} />
            ) : (
              <Link2 size={14} strokeWidth={2.2} />
            )}
          </span>
          <span>{props.block.name}</span>
        </div>
        <div class={attachmentDetailClass}>{getResourceLinkDetail(props.block)}</div>
      </div>
    )
  }

  if (props.message.role === "system") {
    return (
      <div key={`${props.message.id}:block:${props.blockIndex}`} class={systemTextClass}>
        {props.block.text}
      </div>
    )
  }

  return (
    <MarkdownMessage
      key={`${props.message.id}:block:${props.blockIndex}`}
      markdown={props.block.text}
      role={props.message.role}
      streaming={props.message.streaming && props.blockIndex === props.lastTextBlockIndex}
    />
  )
}

/** Renders one text transcript row with Markdown-rich assistant and user bubbles. */
export function TextTranscriptRow(props: { row: MessageListRow<SessionTranscriptTextMessage> }) {
  const message = props.row.item
  const bubbleWidth = getBubbleMaxWidth(props.row.viewportWidth, message)
  const lastTextBlockIndex = message.content.reduce((lastIndex, block, index) => {
    return block.type === "text" ? index : lastIndex
  }, -1)
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
            <div class={transcriptContentClass}>
              {message.content.map((block, blockIndex) =>
                renderContentBlock({
                  block,
                  blockIndex,
                  lastTextBlockIndex,
                  message,
                }),
              )}
            </div>
          </div>
        </div>
      </div>
    </article>
  )
}
