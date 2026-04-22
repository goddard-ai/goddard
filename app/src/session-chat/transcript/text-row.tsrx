import { cx } from "@goddard-ai/styled-system/css"
import { BookOpen, Link2 } from "lucide-react"

import type {
  SessionTranscriptContentBlock,
  SessionTranscriptTextMessage,
} from "~/sessions/models.ts"
import { MarkdownMessage } from "../markdown-message.tsrx"
import type { MessageListRow } from "../message-list.tsrx"
import { getBubbleMaxWidth } from "./layout.ts"
import {
  assistantMessageClass,
  assistantBubbleClass,
  attachmentCardClass,
  attachmentDetailClass,
  attachmentHeadingClass,
  attachmentIconClass,
  bubbleFrameClass,
  rowClass,
  rowColumnClass,
  rowInnerClass,
  transcriptContentClass,
  userBubbleClass,
} from "./styles.ts"

function isSkillResourceLink(block: Extract<SessionTranscriptContentBlock, {
  type: "resource_link";
}>) {
  try {
    return new URL(block.uri).pathname.endsWith("/SKILL.md")
  } catch {
    return block.title?.toLowerCase().endsWith(" skill") ?? false
  }
}

function getResourceLinkDetail(block: Extract<SessionTranscriptContentBlock, {
  type: "resource_link";
}>) {
  if (block.description) {
    return block.description
  }

  if (block.title && block.title !== block.name) {
    return block.title
  }

  return block.uri
}

function renderContentBlock(props: {
  block: SessionTranscriptContentBlock;
  blockIndex: number;
  lastTextBlockIndex: number;
  message: SessionTranscriptTextMessage;
}) {
  if (props.block.type === "resource_link") {
    return <tsx>
      <div key={`${props.message.id}:block:${props.blockIndex}`} class={attachmentCardClass}>
        <div class={attachmentHeadingClass}>
          <span class={attachmentIconClass} aria-hidden="true">
            if (isSkillResourceLink(props.block))
            {<BookOpen size={14} strokeWidth={2.2} />}
            else
            {<Link2 size={14} strokeWidth={2.2} />}
          </span>
          <span>
            {props.block.name}
          </span>
        </div>
        <div class={attachmentDetailClass}>
          {getResourceLinkDetail(props.block)}
        </div>
      </div>
    </tsx>
  }

  const markdownRole =
    props.message.role === "user" ? "user" : "assistant"

  return <tsx>
    <MarkdownMessage key={`${props.message.id}:block:${props.blockIndex}`} markdown={props.block.text} role={markdownRole} streaming={props.message.streaming &&
      props.blockIndex === props.lastTextBlockIndex} />
  </tsx>
}

/** Renders one text transcript row with Markdown-rich assistant and user bubbles. */
export component TextTranscriptRow(props: {
  row: MessageListRow<SessionTranscriptTextMessage>;
}) {
  const message = props.row.item
  const isUserMessage = message.role === "user"
  const lastTextBlockIndex = message.content.reduce((lastIndex, block, index) => {
    return block.type === "text" ? index : lastIndex
  }, -1)
  const alignmentStyle = isUserMessage
    ? { justifyContent: "flex-end" }
    : { justifyContent: "flex-start" }

  if (message.role === "system") {
    return
  }

  <article class={rowClass}>
    <div class={rowInnerClass} style={alignmentStyle}>
      <div
        class={rowColumnClass}
        style={{
          width: isUserMessage
            ? `${getBubbleMaxWidth(props.row.viewportWidth, message)}px`
            : "100%",
        }}
      >
        <div
          class={isUserMessage
            ? cx(bubbleFrameClass, userBubbleClass)
            : cx(assistantMessageClass, assistantBubbleClass)}
        >
          <div class={transcriptContentClass}>
            for (const block of message.content; index blockIndex) {
              {renderContentBlock(
                {
                  block,
                  blockIndex,
                  lastTextBlockIndex,
                  message,
                },
              )}
            }
          </div>
        </div>
      </div>
    </div>
  </article>
}
