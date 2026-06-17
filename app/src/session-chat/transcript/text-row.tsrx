import { cx } from "@goddard-ai/styled-system/css"
import { BookOpen, Check, ChevronDown, Copy, Link2 } from "lucide-react"
import { useEffect, useLayoutEffect, useRef, useState } from "preact/hooks"

import type {
  SessionTranscriptContentBlock,
  SessionTranscriptTextMessage,
} from "~/sessions/models.ts"
import { text } from "~/language/text.ts"
import { GoodTooltip } from "~/lib/good-tooltip.tsrx"
import { MarkdownMessage } from "../markdown-message.tsrx"
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

function CopyMessageMarkdownButton(props: {
  markdown: string;
}) @{
  const [copied, setCopied] = useState(false)
  const resetTimerRef = useRef<number | null>(null)

  useEffect(() => {
    return () => {
      if (resetTimerRef.current !== null) {
        window.clearTimeout(resetTimerRef.current)
      }
    }
  }, [])

  const copyMarkdown = async () => {
    try {
      await navigator.clipboard.writeText(props.markdown)
    } catch (error) {
      console.error("Failed to copy transcript markdown.", error)
      return
    }

    setCopied(true)

    if (resetTimerRef.current !== null) {
      window.clearTimeout(resetTimerRef.current)
    }

    resetTimerRef.current = window.setTimeout(() => {
      setCopied(false)
      resetTimerRef.current = null
    }, 1250)
  }

  <GoodTooltip
    ariaLabel={copied ? "Copied markdown" : "Copy markdown"}
    content={copied ? "Copied" : "Copy markdown"}
    side="top"
  >
    <button
      aria-label={copied ? "Copied message markdown" : "Copy message markdown"}
      class={styles.copyMarkdownButton}
      type="button"
      onClick={copyMarkdown}
    >
      @if (copied) {
        <Check size={14} strokeWidth={2.3} />
      } @else {
        <Copy size={14} strokeWidth={2.3} />
      }
    </button>
  </GoodTooltip>
}

function TranscriptContentBlock(props: {
  block: SessionTranscriptContentBlock;
  blockIndex: number;
  lastTextBlockIndex: number;
  message: SessionTranscriptTextMessage;
}) @{
  if (props.block.type === "resource_link") {
    return <div class={styles.attachmentCard}>
      <div class={styles.attachmentHeading}>
        <span class={styles.attachmentIcon} aria-hidden="true">
          @if (isSkillResourceLink(props.block)) {
            <BookOpen size={14} strokeWidth={2.2} />
          } @else {
            <Link2 size={14} strokeWidth={2.2} />
          }
        </span>
        <span>{props.block.name}</span>
      </div>
      <div class={styles.attachmentDetail}>{getResourceLinkDetail(props.block)}</div>
    </div>
  }

  const markdownRole =
    props.message.role === "user" ? "user" : "assistant"

  <MarkdownMessage
    markdown={props.block.text}
    role={markdownRole}
    streaming={props.message.streaming && props.blockIndex === props.lastTextBlockIndex}
  />
}

function UserPromptContent(props: {
  lastTextBlockIndex: number;
  message: SessionTranscriptTextMessage;
  messageId: string;
}) @{
  const contentRef = useRef<HTMLDivElement | null>(null)
  const [expanded, setExpanded] = useState(false)
  const [clamped, setClamped] = useState(false)

  useEffect(() => {
    setExpanded(false)
  }, [props.messageId])

  useLayoutEffect(() => {
    const contentElement = contentRef.current

    if (!contentElement) {
      return
    }

    const updateClampedState = () => {
      setClamped(contentElement.scrollHeight > contentElement.clientHeight + 1)
    }

    updateClampedState()

    const resizeObserver = new ResizeObserver(updateClampedState)
    resizeObserver.observe(contentElement)

    return () => {
      resizeObserver.disconnect()
    }
  }, [expanded, props.messageId])

  <>
    <div
      ref={contentRef}
      aria-expanded={expanded ? "true" : undefined}
      class={cx(styles.transcriptContent, !expanded && styles.userPromptClampedContent)}
    >
      @for (const block of props.message.content; index blockIndex) {
        <TranscriptContentBlock
          key={`${props.message.id}:block:${blockIndex}`}
          {block}
          {blockIndex}
          lastTextBlockIndex={props.lastTextBlockIndex}
          message={props.message}
        />
      }
    </div>

    @if (clamped && !expanded) {
      <button
        class={styles.seeFullMessageButton}
        type="button"
        onClick={() => {
          setExpanded(true)
        }}
      >
        <span>{text.seeFullMessage}</span>
        <ChevronDown size={15} strokeWidth={2.2} />
      </button>
    }
  </>
}

/** Renders one text transcript row with Markdown-rich assistant and user bubbles. */
export function TextTranscriptRow(props: {
  row: {
    item: SessionTranscriptTextMessage;
    index: number;
    viewportWidth: number;
  };
}) @{
  const message = props.row.item
  const isUserMessage = message.role === "user"
  const lastTextBlockIndex = message.content.reduce((lastIndex, block, index) => {
    return block.type === "text" ? index : lastIndex
  }, -1)
  const alignmentStyle = isUserMessage
    ? { justifyContent: "flex-end" }
    : { justifyContent: "flex-start" }
  const rawMarkdown = message.content.flatMap(
    (block) => block.type === "text" ? [block.text] : [],
  ).join("\n\n")

  if (message.role === "system") {
    return null
  }

  <article class={styles.row}>
    <div class={styles.rowInner} style={alignmentStyle}>
      <div
        class={cx(styles.rowColumn, styles.messageContainer)}
        style={{
          maxWidth: isUserMessage
            ? `${getBubbleMaxWidth(props.row.viewportWidth, message)}px`
            : undefined,
          width: isUserMessage ? "fit-content" : "100%",
        }}
      >
        <div
          class={isUserMessage
            ? cx(styles.bubbleFrame, styles.userBubble)
            : cx(styles.assistantMessage, styles.assistantBubble)}
        >
          <div class={styles.transcriptContent}>
            @if (isUserMessage) {
              <UserPromptContent {lastTextBlockIndex} {message} messageId={message.id} />
            } @else {
              @for (const block of message.content; index blockIndex) {
                <TranscriptContentBlock
                  key={`${message.id}:block:${blockIndex}`}
                  {block}
                  {blockIndex}
                  {lastTextBlockIndex}
                  {message}
                />
              }
            }
          </div>

          @if (!isUserMessage && rawMarkdown.length > 0) {
            <div data-message-actions class={cx(styles.messageActions, styles.messageActionsLeft)}>
              <CopyMessageMarkdownButton markdown={rawMarkdown} />
            </div>
          }
        </div>

        @if (isUserMessage && rawMarkdown.length > 0) {
          <div data-message-actions class={cx(styles.messageActions, styles.messageActionsRight)}>
            <CopyMessageMarkdownButton markdown={rawMarkdown} />
          </div>
        }
      </div>
    </div>
  </article>
}
