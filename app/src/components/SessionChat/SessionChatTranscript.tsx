import { css, cx } from "@goddard-ai/styled-system/css"
import { token } from "@goddard-ai/styled-system/tokens"
import {
  VirtualizedPretextParagraphList,
  type PretextParagraphMeasurement,
  type VirtualizedPretextParagraphRow,
} from "../Pretext/VirtualizedPretextParagraphList"

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
  position: "absolute",
  insetInline: "0",
  paddingInline: "4px",
})

const rowInnerClass = css({
  display: "flex",
  width: "100%",
})

const metaRowClass = css({
  display: "flex",
  alignItems: "center",
  gap: "10px",
  marginBottom: "8px",
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
const META_HEIGHT = 26
const BUBBLE_PADDING_X = 32
const BUBBLE_PADDING_Y = 28
const ROW_GAP = 18
const VIRTUAL_OVERSCAN_PX = 420
const MIN_TEXT_WIDTH = 144

/** One transcript message rendered in the dumb visual transcript surface. */
export type SessionChatTranscriptMessage = {
  id: string
  role: "assistant" | "user" | "system"
  authorName: string
  timestampLabel: string
  text: string
}

/** Props accepted by the dumb session transcript component. */
export type SessionChatTranscriptProps = {
  messages: readonly SessionChatTranscriptMessage[]
  class?: string
  initialScrollPosition?: "top" | "bottom"
  scrollCacheKey?: string
}

/** Extra row data carried alongside one virtualized transcript paragraph. */
type TranscriptRowData = {
  bubbleWidth: number
}

/** Returns the maximum bubble width available for one message role at the current viewport width. */
function getBubbleMaxWidth(
  viewportWidth: number,
  role: SessionChatTranscriptMessage["role"],
): number {
  const safeViewportWidth = Math.max(280, viewportWidth - 48)

  if (role === "system") {
    return Math.max(240, Math.min(560, safeViewportWidth - 56))
  }

  if (safeViewportWidth < 720) {
    return Math.max(220, safeViewportWidth - 22)
  }

  return Math.max(320, Math.min(720, Math.round(safeViewportWidth * 0.72)))
}

/** Returns the maximum text width available inside one transcript bubble. */
function getTranscriptTextWidth(
  message: SessionChatTranscriptMessage,
  viewportWidth: number,
): number {
  const bubbleMaxWidth = getBubbleMaxWidth(viewportWidth, message.role)
  return Math.max(MIN_TEXT_WIDTH, bubbleMaxWidth - BUBBLE_PADDING_X)
}

/** Builds the transcript bubble metrics from one measured paragraph. */
function getTranscriptRowData(
  message: SessionChatTranscriptMessage,
  viewportWidth: number,
  paragraph: PretextParagraphMeasurement,
): TranscriptRowData {
  const bubbleMaxWidth = getBubbleMaxWidth(viewportWidth, message.role)
  const bubbleWidth = Math.max(
    Math.min(bubbleMaxWidth, paragraph.maxLineWidth + BUBBLE_PADDING_X),
    Math.min(bubbleMaxWidth, 196),
  )

  return {
    bubbleWidth,
  }
}

/** Renders the visible window of one chat transcript using Pretext-driven row layout. */
export function SessionChatTranscript(props: SessionChatTranscriptProps) {
  const effectiveInitialScroll = props.initialScrollPosition ?? "bottom"

  return (
    <div class={cx(transcriptViewportClass, props.class)}>
      <VirtualizedPretextParagraphList<SessionChatTranscriptMessage, TranscriptRowData>
        getParagraphSpec={(message, _index, viewportWidth) => ({
          key: message.id,
          text: message.text,
          font: BODY_FONT,
          lineHeight: BODY_LINE_HEIGHT,
          maxWidth: getTranscriptTextWidth(message, viewportWidth),
          whiteSpace: "pre-wrap",
        })}
        initialScrollPosition={effectiveInitialScroll}
        items={props.messages}
        layoutRow={({ item, paragraph, viewportWidth }) => ({
          height: META_HEIGHT + BUBBLE_PADDING_Y + paragraph.height + ROW_GAP,
          data: getTranscriptRowData(item, viewportWidth, paragraph),
        })}
        loadingFallback={<div class={loadingStateClass}>Preparing transcript layout...</div>}
        overscanPx={VIRTUAL_OVERSCAN_PX}
        renderRow={(row) => <TranscriptRow row={row} />}
        scrollCacheKey={props.scrollCacheKey}
      />
    </div>
  )
}

/** Renders one positioned transcript row with manual line rendering that matches Pretext layout. */
function TranscriptRow(props: {
  row: VirtualizedPretextParagraphRow<SessionChatTranscriptMessage, TranscriptRowData>
}) {
  const alignmentStyle =
    props.row.item.role === "user"
      ? { justifyContent: "flex-end" }
      : props.row.item.role === "system"
        ? { justifyContent: "center" }
        : { justifyContent: "flex-start" }

  const metaAlignmentStyle =
    props.row.item.role === "user"
      ? { justifyContent: "flex-end" }
      : props.row.item.role === "system"
        ? { justifyContent: "center" }
        : { justifyContent: "flex-start" }

  const bubbleClass =
    props.row.item.role === "user"
      ? userBubbleClass
      : props.row.item.role === "system"
        ? systemBubbleClass
        : assistantBubbleClass

  const lineClass =
    props.row.item.role === "user"
      ? userLineClass
      : props.row.item.role === "system"
        ? systemLineClass
        : assistantLineClass

  return (
    <article class={rowClass} style={{ top: `${props.row.top}px` }}>
      <div class={rowInnerClass} style={alignmentStyle}>
        <div style={{ width: `${props.row.data.bubbleWidth}px` }}>
          <div class={metaRowClass} style={metaAlignmentStyle}>
            <span class={metaAuthorClass}>{props.row.item.authorName}</span>
            <span class={metaTimestampClass}>{props.row.item.timestampLabel}</span>
          </div>
          <div class={cx(bubbleFrameClass, bubbleClass)}>
            {props.row.paragraph.lines.map((line, index) => (
              <div key={`${props.row.item.id}:${index}`} class={cx(transcriptLineClass, lineClass)}>
                {line.text.length > 0 ? line.text : "\u00a0"}
              </div>
            ))}
          </div>
        </div>
      </div>
    </article>
  )
}
