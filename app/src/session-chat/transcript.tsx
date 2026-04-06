import {
  layoutWithLines,
  prepareWithSegments,
  type LayoutLine,
  type PrepareOptions,
  type PreparedTextWithSegments,
} from "@chenglou/pretext"
import { css, cx } from "@goddard-ai/styled-system/css"
import { token } from "@goddard-ai/styled-system/tokens"
import { useMemo } from "preact/hooks"
import { MessageList, type MessageListRow } from "./message-list"

const transcriptViewportClass = css({
  position: "relative",
  minHeight: "100%",
  paddingInline: "20px",
  paddingBlock: "26px 32px",
  background:
    `radial-gradient(circle at top right, color-mix(in srgb, ${token.var("colors.accent")} 10%, transparent), transparent 26%), ` +
    `linear-gradient(180deg, ${token.var("colors.background")} 0%, ${token.var("colors.surface")} 100%)`,
})

const loadingStateClass = css({
  position: "absolute",
  inset: "0",
  display: "grid",
  placeItems: "center",
  color: "muted",
  fontSize: "0.94rem",
  letterSpacing: "0.01em",
  pointerEvents: "none",
})

const rowClass = css({
  paddingInline: "4px",
  paddingBottom: "18px",
})

const rowInnerClass = css({
  display: "flex",
  width: "100%",
})

const rowColumnClass = css({
  display: "grid",
  gap: "8px",
})

const metaRowClass = css({
  display: "flex",
  alignItems: "center",
  gap: "10px",
  color: "muted",
  fontSize: "0.73rem",
  fontWeight: "680",
  letterSpacing: "0.08em",
  textTransform: "uppercase",
})

const metaAuthorClass = css({
  color: "text",
})

const metaTimestampClass = css({
  letterSpacing: "0.04em",
})

const bubbleFrameClass = css({
  display: "inline-flex",
  flexDirection: "column",
  borderRadius: "22px",
  border: "1px solid",
  padding: "14px 16px",
  boxShadow: "0 18px 36px rgba(112, 128, 145, 0.08)",
})

const assistantBubbleClass = css({
  borderColor: "border",
  background: `linear-gradient(180deg, ${token.var("colors.panel")} 0%, ${token.var("colors.background")} 100%)`,
  color: "text",
})

const userBubbleClass = css({
  borderColor: "accent",
  background: `linear-gradient(180deg, color-mix(in srgb, ${token.var("colors.accentStrong")} 24%, white), color-mix(in srgb, ${token.var("colors.accent")} 22%, white))`,
  color: "#102030",
})

const systemBubbleClass = css({
  borderColor: "border",
  background: `linear-gradient(180deg, color-mix(in srgb, ${token.var("colors.surface")} 70%, white), ${token.var("colors.surface")})`,
  color: "muted",
  boxShadow: `inset 0 0 0 1px color-mix(in srgb, ${token.var("colors.border")} 70%, white)`,
})

const transcriptLineClass = css({
  fontFamily: '"SF Pro Text", "Segoe UI", sans-serif',
  fontSize: "15px",
  fontWeight: "450",
  lineHeight: "24px",
  letterSpacing: "0.01em",
  whiteSpace: "pre",
})

const userLineClass = css({
  color: "#102030",
})

const assistantLineClass = css({
  color: "text",
})

const systemLineClass = css({
  color: "muted",
})

const BODY_FONT = '450 15px "SF Pro Text", "Segoe UI", sans-serif'
const BODY_LINE_HEIGHT = 24
const META_HEIGHT = 32
const BUBBLE_PADDING_X = 32
const BUBBLE_PADDING_Y = 28
const ROW_GAP = 18
const VIRTUAL_OVERSCAN_PX = 420
const MIN_TEXT_WIDTH = 144
const DEFAULT_ROW_HEIGHT = 132
const NARROW_BUBBLE_WIDTH_BREAKPOINT = 520
const WIDE_BUBBLE_WIDTH_BREAKPOINT = 760

/** Linearly interpolates one value within an inclusive range. */
function lerp(start: number, end: number, progress: number): number {
  return start + (end - start) * progress
}

const preparedParagraphCache = new Map<string, PreparedTextWithSegments>()

/** One measured paragraph produced from Pretext layout. */
type PretextParagraphMeasurement = {
  lines: readonly LayoutLine[]
  lineCount: number
  height: number
  maxLineWidth: number
}

/** One transcript message rendered in the dumb visual transcript surface. */
export type TranscriptMessage = {
  id: string
  role: "assistant" | "user" | "system"
  authorName: string
  timestampLabel: string
  text: string
}

/** Props accepted by the dumb session transcript component. */
export type TranscriptProps = {
  messages: readonly TranscriptMessage[]
  class?: string
  initialScrollPosition?: "top" | "bottom"
  scrollCacheKey?: string
}

/** Returns the cached prepared representation for one transcript paragraph. */
function prepareParagraph(
  text: string,
  font: string,
  whiteSpace?: PrepareOptions["whiteSpace"],
): PreparedTextWithSegments {
  const cacheKey = `${font}::${whiteSpace ?? "normal"}::${text}`
  const cachedPrepared = preparedParagraphCache.get(cacheKey)

  if (cachedPrepared) {
    return cachedPrepared
  }

  const prepared = prepareWithSegments(text, font, {
    whiteSpace,
  })
  preparedParagraphCache.set(cacheKey, prepared)
  return prepared
}

/** Measures one transcript paragraph for the current width budget. */
function measureParagraph(text: string, maxWidth: number): PretextParagraphMeasurement {
  const prepared = prepareParagraph(text, BODY_FONT, "pre-wrap")
  const lineLayout = layoutWithLines(prepared, maxWidth, BODY_LINE_HEIGHT)
  const maxLineWidth = lineLayout.lines.reduce((widest, line) => Math.max(widest, line.width), 0)

  return {
    lines: lineLayout.lines,
    lineCount: lineLayout.lineCount,
    height: lineLayout.height,
    maxLineWidth,
  }
}

/** Returns the maximum bubble width available for one message role at the current viewport width. */
function getBubbleMaxWidth(
  viewportWidth: number,
  role: TranscriptMessage["role"],
): number {
  const safeViewportWidth = Math.max(280, viewportWidth - 48)

  if (role === "system") {
    return Math.max(240, Math.min(560, safeViewportWidth - 56))
  }

  const narrowWidth = Math.max(220, safeViewportWidth - 22)
  const wideWidth = Math.max(320, Math.min(720, Math.round(safeViewportWidth * 0.72)))

  if (safeViewportWidth <= NARROW_BUBBLE_WIDTH_BREAKPOINT) {
    return narrowWidth
  }

  if (safeViewportWidth >= WIDE_BUBBLE_WIDTH_BREAKPOINT) {
    return wideWidth
  }

  const progress =
    (safeViewportWidth - NARROW_BUBBLE_WIDTH_BREAKPOINT) /
    (WIDE_BUBBLE_WIDTH_BREAKPOINT - NARROW_BUBBLE_WIDTH_BREAKPOINT)

  return Math.round(lerp(narrowWidth, wideWidth, progress))
}

/** Returns the maximum text width available inside one transcript bubble. */
function getTranscriptTextWidth(message: TranscriptMessage, viewportWidth: number): number {
  const bubbleMaxWidth = getBubbleMaxWidth(viewportWidth, message.role)
  return Math.max(MIN_TEXT_WIDTH, bubbleMaxWidth - BUBBLE_PADDING_X)
}

/** Builds the rendered transcript bubble width from one measured paragraph. */
function getTranscriptBubbleWidth(
  message: TranscriptMessage,
  viewportWidth: number,
  paragraph: PretextParagraphMeasurement,
): number {
  const bubbleMaxWidth = getBubbleMaxWidth(viewportWidth, message.role)

  return Math.max(
    Math.min(bubbleMaxWidth, paragraph.maxLineWidth + BUBBLE_PADDING_X),
    Math.min(bubbleMaxWidth, 196),
  )
}

/** Rough row estimate used by Virtuoso before the real transcript row is measured. */
function estimateTranscriptRowHeight(message: TranscriptMessage, viewportWidth: number): number {
  const textWidth = getTranscriptTextWidth(message, viewportWidth)
  const approximateCharactersPerLine = Math.max(12, Math.floor(textWidth / 7.6))
  const normalizedLength = message.text.replace(/\s+/g, " ").trim().length
  const explicitBreakCount = Math.max(0, message.text.split("\n").length - 1)
  const approximateLineCount = Math.max(
    1,
    Math.ceil(normalizedLength / approximateCharactersPerLine) + explicitBreakCount,
  )

  return META_HEIGHT + BUBBLE_PADDING_Y + approximateLineCount * BODY_LINE_HEIGHT + ROW_GAP
}

/** Renders one chat transcript using Pretext paragraphs inside a Virtuoso row virtualizer. */
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

/** Renders one transcript row with manual line rendering that matches Pretext layout. */
function TranscriptRow(props: { row: MessageListRow<TranscriptMessage> }) {
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

  const metaAlignmentStyle =
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
          <div class={metaRowClass} style={metaAlignmentStyle}>
            <span class={metaAuthorClass}>{message.authorName}</span>
            <span class={metaTimestampClass}>{message.timestampLabel}</span>
          </div>
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
