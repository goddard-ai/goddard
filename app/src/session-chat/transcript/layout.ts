import {
  layoutWithLines,
  prepareWithSegments,
  type LayoutLine,
  type PrepareOptions,
  type PreparedTextWithSegments,
} from "@chenglou/pretext"

import type {
  SessionTranscriptItem,
  SessionTranscriptTextMessage,
  SessionTranscriptToolContent,
} from "~/sessions/models.ts"
import {
  ATTACHMENT_ROW_HEIGHT,
  BODY_FONT,
  BODY_LINE_HEIGHT,
  BUBBLE_PADDING_X,
  BUBBLE_PADDING_Y,
  CONTENT_BLOCK_GAP,
  META_HEIGHT,
  MIN_TEXT_WIDTH,
  NARROW_BUBBLE_WIDTH_BREAKPOINT,
  ROW_GAP,
  TOOL_DIFF_PREVIEW_LINE_LIMIT,
  WIDE_BUBBLE_WIDTH_BREAKPOINT,
} from "./styles.ts"

const preparedParagraphCache = new Map<string, PreparedTextWithSegments>()

/** One measured paragraph produced from Pretext layout. */
export type PretextParagraphMeasurement = {
  lines: readonly LayoutLine[]
  lineCount: number
  height: number
  maxLineWidth: number
}

export function isTextMessage(
  message: SessionTranscriptItem,
): message is SessionTranscriptTextMessage {
  return message.kind === "message"
}

/** Linearly interpolates one value within an inclusive range. */
function lerp(start: number, end: number, progress: number) {
  return start + (end - start) * progress
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
export function measureParagraph(text: string, maxWidth: number): PretextParagraphMeasurement {
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
export function getBubbleMaxWidth(viewportWidth: number, message: SessionTranscriptItem) {
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
export function getTranscriptTextWidth(message: SessionTranscriptItem, viewportWidth: number) {
  const bubbleMaxWidth = getBubbleMaxWidth(viewportWidth, message)
  return Math.max(MIN_TEXT_WIDTH, bubbleMaxWidth - BUBBLE_PADDING_X)
}

/** Builds the rendered transcript bubble width from one measured paragraph. */
export function getTranscriptBubbleWidth(
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

function estimateTextMessageBlockHeight(
  block: SessionTranscriptTextMessage["content"][number],
  maxWidth: number,
) {
  if (block.type === "resource_link") {
    return ATTACHMENT_ROW_HEIGHT
  }

  return estimateLineCount(block.text, maxWidth) * BODY_LINE_HEIGHT
}

export function buildToolDiffPreview(
  content: Extract<SessionTranscriptToolContent, { type: "diff" }>,
) {
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
export function estimateTranscriptRowHeight(message: SessionTranscriptItem, viewportWidth: number) {
  const textWidth = getTranscriptTextWidth(message, viewportWidth)

  if (isTextMessage(message)) {
    const contentHeight = message.content.reduce((height, block, index) => {
      return (
        height +
        estimateTextMessageBlockHeight(block, textWidth) +
        (index < message.content.length - 1 ? CONTENT_BLOCK_GAP : 0)
      )
    }, 0)

    return META_HEIGHT + BUBBLE_PADDING_Y + Math.max(contentHeight, BODY_LINE_HEIGHT) + ROW_GAP
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
        `${location.path}${location.line != null ? `:${location.line}` : ""}`,
        textWidth,
      )
    }
  }

  return META_HEIGHT + BUBBLE_PADDING_Y + approximateLineCount * BODY_LINE_HEIGHT + ROW_GAP + 28
}
