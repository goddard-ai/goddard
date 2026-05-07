import { cx } from "@goddard-ai/styled-system/css"
import { BookOpen, Link2 } from "lucide-react"

import type {
  SessionTranscriptContentBlock,
  SessionTranscriptTextMessage,
} from "~/sessions/models.ts"
import { MarkdownMessage } from "../markdown-message.tsrx"
import type { MessageListRow } from "../message-list.tsrx"
import { getBubbleMaxWidth } from "./layout.ts"
import styles from "./transcript.style.ts"

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

component TranscriptContentBlock(props: {
  block: SessionTranscriptContentBlock;
  blockIndex: number;
  lastTextBlockIndex: number;
  message: SessionTranscriptTextMessage;
}) {
  if (props.block.type === "resource_link") {
    <div class={styles.attachmentCard}>
      <div class={styles.attachmentHeading}>
        <span class={styles.attachmentIcon} aria-hidden="true">
          if (isSkillResourceLink(props.block)) {
            <BookOpen size={14} strokeWidth={2.2} />
          } else {
            <Link2 size={14} strokeWidth={2.2} />
          }
        </span>
        <span>
          {props.block.name}
        </span>
      </div>
      <div class={styles.attachmentDetail}>
        {getResourceLinkDetail(props.block)}
      </div>
    </div>
  } else {
    const markdownRole =
      props.message.role === "user" ? "user" : "assistant"

    <MarkdownMessage
      markdown={props.block.text}
      role={markdownRole}
      streaming={props.message.streaming && props.blockIndex === props.lastTextBlockIndex}
    />
  }
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

  <article class={styles.row}>
    <div class={styles.rowInner} style={alignmentStyle}>
      <div
        class={styles.rowColumn}
        style={{
          width: isUserMessage
            ? `${getBubbleMaxWidth(props.row.viewportWidth, message)}px`
            : "100%",
        }}
      >
        <div
          class={isUserMessage
            ? cx(styles.bubbleFrame, styles.userBubble)
            : cx(styles.assistantMessage, styles.assistantBubble)}
        >
          <div class={styles.transcriptContent}>
            for (const block of message.content; index blockIndex) {
              <TranscriptContentBlock
                key={`${message.id}:block:${blockIndex}`}
                {block}
                {blockIndex}
                {lastTextBlockIndex}
                {message}
              />
            }
          </div>
        </div>
      </div>
    </div>
  </article>
}
