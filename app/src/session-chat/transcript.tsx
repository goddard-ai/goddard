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
import { MessageList, type MessageListRow } from "./message-list.tsx"
import type {
  SessionTranscriptItem,
  SessionTranscriptTextMessage,
  SessionTranscriptToolCall,
  SessionTranscriptToolContent,
  SessionTranscriptToolStatus,
} from "~/sessions/models.ts"

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
  background:
    `linear-gradient(180deg, color-mix(in srgb, ${token.var("colors.accentStrong")} 72%, ${token.var("colors.background")}), ` +
    `color-mix(in srgb, ${token.var("colors.accent")} 82%, ${token.var("colors.background")}))`,
  color: "accentFg",
})

const systemBubbleClass = css({
  borderColor: "border",
  background:
    `linear-gradient(180deg, color-mix(in srgb, ${token.var("colors.surface")} 72%, ${token.var("colors.background")}), ` +
    `${token.var("colors.surface")})`,
  color: "muted",
  boxShadow: `inset 0 0 0 1px color-mix(in srgb, ${token.var("colors.border")} 72%, ${token.var("colors.background")})`,
})

const toolBubbleClass = css({
  borderColor: "border",
  background:
    `radial-gradient(circle at top right, color-mix(in srgb, ${token.var("colors.accent")} 8%, transparent), transparent 34%), ` +
    `linear-gradient(180deg, color-mix(in srgb, ${token.var("colors.panel")} 92%, white), ${token.var("colors.background")})`,
  color: "text",
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
  color: "accentFg",
})

const assistantLineClass = css({
  color: "text",
})

const systemLineClass = css({
  color: "muted",
})

const toolCardClass = css({
  display: "grid",
  gap: "14px",
})

const toolHeaderClass = css({
  display: "flex",
  alignItems: "flex-start",
  justifyContent: "space-between",
  gap: "12px",
  flexWrap: "wrap",
})

const toolTitleClass = css({
  color: "text",
  fontSize: "0.98rem",
  fontWeight: "700",
  letterSpacing: "-0.01em",
  lineHeight: "1.35",
})

const toolBadgeRowClass = css({
  display: "flex",
  flexWrap: "wrap",
  gap: "8px",
})

const toolBadgeClass = css({
  display: "inline-flex",
  alignItems: "center",
  minHeight: "24px",
  paddingInline: "10px",
  borderRadius: "999px",
  border: "1px solid",
  fontSize: "0.72rem",
  fontWeight: "700",
  letterSpacing: "0.08em",
  textTransform: "uppercase",
  whiteSpace: "nowrap",
})

const toolKindBadgeClass = css({
  borderColor: "border",
  backgroundColor: "surface",
  color: "muted",
})

const toolPendingBadgeClass = css({
  borderColor: "border",
  backgroundColor: "surface",
  color: "muted",
})

const toolRunningBadgeClass = css({
  borderColor: "accent",
  background: `color-mix(in srgb, ${token.var("colors.accent")} 12%, white)`,
  color: "accentStrong",
})

const toolCompletedBadgeClass = css({
  borderColor: "accentStrong",
  background: `color-mix(in srgb, ${token.var("colors.accentStrong")} 16%, white)`,
  color: "accentStrong",
})

const toolFailedBadgeClass = css({
  borderColor: "danger",
  background: `color-mix(in srgb, ${token.var("colors.danger")} 14%, white)`,
  color: "danger",
})

const toolBodyClass = css({
  display: "grid",
  gap: "12px",
})

const toolSectionClass = css({
  display: "grid",
  gap: "8px",
})

const toolSectionLabelClass = css({
  color: "muted",
  fontSize: "0.68rem",
  fontWeight: "700",
  letterSpacing: "0.12em",
  textTransform: "uppercase",
})

const toolTextClass = css({
  color: "text",
  fontFamily: '"SF Pro Text", "Segoe UI", sans-serif',
  fontSize: "0.94rem",
  fontWeight: "450",
  lineHeight: "1.65",
  letterSpacing: "0.01em",
  whiteSpace: "pre-wrap",
})

const toolDiffClass = css({
  display: "grid",
  gap: "8px",
  padding: "12px 14px",
  borderRadius: "16px",
  border: "1px solid",
  borderColor: "border",
  backgroundColor: "background",
})

const toolDiffPathClass = css({
  color: "text",
  fontFamily: '"SF Mono", "Monaco", monospace',
  fontSize: "0.83rem",
  fontWeight: "620",
  lineHeight: "1.5",
})

const toolDiffPreviewClass = css({
  color: "text",
  fontFamily: '"SF Mono", "Monaco", monospace',
  fontSize: "0.82rem",
  lineHeight: "1.6",
  whiteSpace: "pre-wrap",
})

const toolTerminalClass = css({
  display: "grid",
  gap: "6px",
  padding: "12px 14px",
  borderRadius: "16px",
  border: "1px dashed",
  borderColor: "border",
  backgroundColor: "background",
})

const toolTerminalLabelClass = css({
  color: "muted",
  fontSize: "0.78rem",
  fontWeight: "700",
  letterSpacing: "0.08em",
  textTransform: "uppercase",
})

const toolTerminalIdClass = css({
  color: "text",
  fontFamily: '"SF Mono", "Monaco", monospace',
  fontSize: "0.84rem",
  lineHeight: "1.6",
})

const toolLocationListClass = css({
  display: "grid",
  gap: "8px",
})

const toolLocationRowClass = css({
  display: "flex",
  flexWrap: "wrap",
  gap: "8px",
  alignItems: "baseline",
})

const toolLocationPathClass = css({
  color: "text",
  fontFamily: '"SF Mono", "Monaco", monospace',
  fontSize: "0.84rem",
  lineHeight: "1.6",
})

const toolLocationLineClass = css({
  color: "muted",
  fontSize: "0.82rem",
  lineHeight: "1.6",
})

const toolEmptyStateClass = css({
  color: "muted",
  fontSize: "0.88rem",
  lineHeight: "1.6",
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
const TOOL_DIFF_PREVIEW_LINE_LIMIT = 6

/** Linearly interpolates one value within an inclusive range. */
function lerp(start: number, end: number, progress: number) {
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

/** One transcript row rendered by the session chat surface. */
export type TranscriptMessage = SessionTranscriptItem

/** Props accepted by the dumb session transcript component. */
export type TranscriptProps = {
  messages: readonly TranscriptMessage[]
  class?: string
  initialScrollPosition?: "top" | "bottom"
  scrollCacheKey?: string
}

function isTextMessage(message: TranscriptMessage): message is SessionTranscriptTextMessage {
  return message.kind === "message"
}

function getToolStatusBadgeClass(status: SessionTranscriptToolStatus) {
  if (status === "completed") {
    return toolCompletedBadgeClass
  }

  if (status === "failed") {
    return toolFailedBadgeClass
  }

  if (status === "in_progress") {
    return toolRunningBadgeClass
  }

  return toolPendingBadgeClass
}

function formatToolKindLabel(toolKind: SessionTranscriptToolCall["toolKind"]) {
  return toolKind === "switch_mode"
    ? "Switch mode"
    : `${toolKind.slice(0, 1).toUpperCase()}${toolKind.slice(1)}`
}

function formatToolStatusLabel(status: SessionTranscriptToolStatus) {
  return status === "in_progress"
    ? "Running"
    : `${status.slice(0, 1).toUpperCase()}${status.slice(1)}`
}

/** Returns the cached prepared representation for one transcript paragraph. */
function prepareParagraph(text: string, font: string, whiteSpace?: PrepareOptions["whiteSpace"]) {
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

/** Returns the maximum bubble width available for one transcript row at the current viewport width. */
function getBubbleMaxWidth(viewportWidth: number, message: TranscriptMessage) {
  const safeViewportWidth = Math.max(280, viewportWidth - 48)

  if (isTextMessage(message) && message.role === "system") {
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
function getTranscriptTextWidth(message: TranscriptMessage, viewportWidth: number) {
  const bubbleMaxWidth = getBubbleMaxWidth(viewportWidth, message)
  return Math.max(MIN_TEXT_WIDTH, bubbleMaxWidth - BUBBLE_PADDING_X)
}

/** Builds the rendered transcript bubble width from one measured paragraph. */
function getTranscriptBubbleWidth(
  message: SessionTranscriptTextMessage,
  viewportWidth: number,
  paragraph: PretextParagraphMeasurement,
) {
  const bubbleMaxWidth = getBubbleMaxWidth(viewportWidth, message)

  return Math.max(
    Math.min(bubbleMaxWidth, paragraph.maxLineWidth + BUBBLE_PADDING_X),
    Math.min(bubbleMaxWidth, 196),
  )
}

function estimateLineCount(text: string, maxWidth: number) {
  const approximateCharactersPerLine = Math.max(12, Math.floor(maxWidth / 7.6))
  const normalizedLength = text.replace(/\s+/g, " ").trim().length
  const explicitBreakCount = Math.max(0, text.split("\n").length - 1)

  return Math.max(
    1,
    Math.ceil(normalizedLength / approximateCharactersPerLine) + explicitBreakCount,
  )
}

function buildToolDiffPreview(content: Extract<SessionTranscriptToolContent, { type: "diff" }>) {
  const previewLines: string[] = []
  const oldLines = content.oldText?.split("\n").filter(Boolean) ?? []
  const newLines = content.newText?.split("\n").filter(Boolean) ?? []

  for (const line of oldLines.slice(0, Math.ceil(TOOL_DIFF_PREVIEW_LINE_LIMIT / 2))) {
    previewLines.push(`- ${line}`)
  }

  for (const line of newLines.slice(0, Math.ceil(TOOL_DIFF_PREVIEW_LINE_LIMIT / 2))) {
    previewLines.push(`+ ${line}`)
  }

  if (oldLines.length + newLines.length > TOOL_DIFF_PREVIEW_LINE_LIMIT) {
    previewLines.push("…")
  }

  return previewLines.join("\n").trim() || "Patch content available."
}

function getToolContentPreview(content: SessionTranscriptToolContent) {
  if (content.type === "content") {
    return content.text?.trim() || "Structured tool output."
  }

  if (content.type === "diff") {
    return `${content.path ?? "Edited file"}\n${buildToolDiffPreview(content)}`
  }

  return `Terminal: ${content.terminalId}`
}

/** Rough row estimate used by Virtuoso before the real transcript row is measured. */
function estimateTranscriptRowHeight(message: TranscriptMessage, viewportWidth: number) {
  const textWidth = getTranscriptTextWidth(message, viewportWidth)

  if (isTextMessage(message)) {
    const approximateLineCount = estimateLineCount(message.text, textWidth)

    return META_HEIGHT + BUBBLE_PADDING_Y + approximateLineCount * BODY_LINE_HEIGHT + ROW_GAP
  }

  let approximateLineCount = 2

  approximateLineCount += estimateLineCount(message.title, textWidth)

  for (const content of message.content) {
    approximateLineCount += 1
    approximateLineCount += estimateLineCount(getToolContentPreview(content), textWidth)
  }

  if (message.locations.length > 0) {
    approximateLineCount += 1
    for (const location of message.locations) {
      approximateLineCount += estimateLineCount(
        `${location.path}${location.line ? `:${location.line}` : ""}`,
        textWidth,
      )
    }
  }

  return META_HEIGHT + BUBBLE_PADDING_Y + approximateLineCount * BODY_LINE_HEIGHT + ROW_GAP + 28
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

function renderMetaRow(props: {
  authorName: string
  timestampLabel: string
  alignmentStyle: { justifyContent: string }
}) {
  return (
    <div class={metaRowClass} style={props.alignmentStyle}>
      <span class={metaAuthorClass}>{props.authorName}</span>
      <span class={metaTimestampClass}>{props.timestampLabel}</span>
    </div>
  )
}

/** Renders one text transcript row with manual line rendering that matches Pretext layout. */
function TextTranscriptRow(props: { row: MessageListRow<SessionTranscriptTextMessage> }) {
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
          {renderMetaRow({
            authorName: message.authorName,
            timestampLabel: message.timestampLabel,
            alignmentStyle,
          })}
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

function renderToolContent(content: SessionTranscriptToolContent, key: string) {
  if (content.type === "content") {
    return (
      <section key={key} class={toolSectionClass}>
        <span class={toolSectionLabelClass}>Output</span>
        <div class={toolTextClass}>{content.text?.trim() || "Structured tool output."}</div>
      </section>
    )
  }

  if (content.type === "diff") {
    return (
      <section key={key} class={toolSectionClass}>
        <span class={toolSectionLabelClass}>Diff</span>
        <div class={toolDiffClass}>
          {content.path ? <div class={toolDiffPathClass}>{content.path}</div> : null}
          <div class={toolDiffPreviewClass}>{buildToolDiffPreview(content)}</div>
        </div>
      </section>
    )
  }

  return (
    <section key={key} class={toolSectionClass}>
      <span class={toolSectionLabelClass}>Terminal</span>
      <div class={toolTerminalClass}>
        <span class={toolTerminalLabelClass}>Terminal Session</span>
        <span class={toolTerminalIdClass}>{content.terminalId}</span>
      </div>
    </section>
  )
}

function renderToolLocations(locations: SessionTranscriptToolCall["locations"]) {
  if (locations.length === 0) {
    return null
  }

  return (
    <section class={toolSectionClass}>
      <span class={toolSectionLabelClass}>Locations</span>
      <div class={toolLocationListClass}>
        {locations.map((location, index) => (
          <div
            key={`${location.path}:${location.line ?? "none"}:${index}`}
            class={toolLocationRowClass}
          >
            <span class={toolLocationPathClass}>{location.path}</span>
            {location.line != null ? (
              <span class={toolLocationLineClass}>Line {location.line}</span>
            ) : null}
          </div>
        ))}
      </div>
    </section>
  )
}

/** Renders one transcript tool row with dedicated status and payload sections. */
function ToolTranscriptRow(props: { row: MessageListRow<SessionTranscriptToolCall> }) {
  const message = props.row.item
  const bubbleWidth = getBubbleMaxWidth(props.row.viewportWidth, message)

  return (
    <article class={rowClass}>
      <div class={rowInnerClass} style={{ justifyContent: "flex-start" }}>
        <div class={rowColumnClass} style={{ width: `${bubbleWidth}px` }}>
          {renderMetaRow({
            authorName: message.authorName,
            timestampLabel: message.timestampLabel,
            alignmentStyle: { justifyContent: "flex-start" },
          })}
          <div class={cx(bubbleFrameClass, toolBubbleClass)}>
            <div class={toolCardClass}>
              <div class={toolHeaderClass}>
                <div class={toolTitleClass}>{message.title}</div>
                <div class={toolBadgeRowClass}>
                  <span class={cx(toolBadgeClass, toolKindBadgeClass)}>
                    {formatToolKindLabel(message.toolKind)}
                  </span>
                  <span class={cx(toolBadgeClass, getToolStatusBadgeClass(message.status))}>
                    {formatToolStatusLabel(message.status)}
                  </span>
                </div>
              </div>
              {message.content.length > 0 || message.locations.length > 0 ? (
                <div class={toolBodyClass}>
                  {message.content.map((content, index) =>
                    renderToolContent(content, `${message.id}:content:${index}`),
                  )}
                  {renderToolLocations(message.locations)}
                </div>
              ) : (
                <div class={toolEmptyStateClass}>No tool output was attached to this call.</div>
              )}
            </div>
          </div>
        </div>
      </div>
    </article>
  )
}

/** Renders one transcript row using the row-specific presentation path. */
function TranscriptRow(props: { row: MessageListRow<TranscriptMessage> }) {
  if (props.row.item.kind === "toolCall") {
    return <ToolTranscriptRow row={props.row as MessageListRow<SessionTranscriptToolCall>} />
  }

  return <TextTranscriptRow row={props.row as MessageListRow<SessionTranscriptTextMessage>} />
}
