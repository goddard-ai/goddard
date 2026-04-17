import { css } from "@goddard-ai/styled-system/css"

export const transcriptViewportClass = css({
  position: "relative",
  minHeight: "100%",
  paddingInline: "20px",
  paddingTop: "24px",
  paddingBottom: "32px",
})

export const loadingStateClass = css({
  position: "absolute",
  inset: "0",
  display: "grid",
  placeItems: "center",
  color: "fg.muted",
  fontSize: "0.94rem",
  letterSpacing: "0.01em",
  pointerEvents: "none",
})

export const rowClass = css({
  paddingBottom: "16px",
})

export const rowInnerClass = css({
  display: "flex",
  width: "100%",
})

export const rowColumnClass = css({
  display: "grid",
  gap: "0",
})

export const bubbleFrameClass = css({
  display: "inline-flex",
  flexDirection: "column",
  borderRadius: "22px",
  border: "1px solid",
  padding: "14px 16px",
})

export const assistantMessageClass = css({
  display: "grid",
  width: "100%",
  color: "fg.default",
})

export const assistantBubbleClass = css({
  color: "fg.default",
})

export const userBubbleClass = css({
  borderColor: "transcript.userBubble.border",
  backgroundColor: "transcript.userBubble.end",
  color: "fg.default",
})

export const transcriptContentClass = css({
  display: "grid",
  gap: "10px",
  minWidth: 0,
})

export const attachmentCardClass = css({
  display: "grid",
  gap: "6px",
  minWidth: 0,
  padding: "2px 0",
})

export const attachmentHeadingClass = css({
  display: "flex",
  alignItems: "center",
  gap: "8px",
  color: "fg.default",
  fontSize: "0.86rem",
  fontWeight: "680",
  lineHeight: "1.35",
})

export const attachmentIconClass = css({
  display: "inline-flex",
  color: "fg.muted",
})

export const attachmentDetailClass = css({
  color: "fg.muted",
  fontSize: "0.8rem",
  lineHeight: "1.45",
  overflowWrap: "anywhere",
  wordBreak: "break-word",
})

export const toolDisclosureClass = css({
  display: "grid",
  gap: "10px",
  width: "100%",
})

export const toolSummaryClass = css({
  display: "flex",
  flexWrap: "wrap",
  alignItems: "baseline",
  gap: "8px",
  width: "100%",
  color: "fg.muted",
  listStyle: "none",
  outline: "none",
  textAlign: "left",
  cursor: "pointer",
  "&::marker": {
    content: '""',
  },
  "&::-webkit-details-marker": {
    display: "none",
  },
})

export const toolSummaryTextClass = css({
  color: "fg.default",
  fontSize: "0.9rem",
  fontWeight: "580",
  lineHeight: "1.5",
})

export const toolToggleHintClass = css({
  color: "fg.muted",
  fontSize: "0.82rem",
  lineHeight: "1.5",
  textDecoration: "underline",
  textUnderlineOffset: "0.18em",
})

export const toolDetailListClass = css({
  display: "grid",
  gap: "12px",
  margin: "0",
  padding: "0",
  listStyle: "none",
})

export const toolDetailItemClass = css({
  display: "grid",
  gap: "6px",
  paddingInlineStart: "12px",
  listStyle: "none",
})

export const toolDetailHeadingClass = css({
  color: "fg.default",
  fontSize: "0.86rem",
  fontWeight: "620",
  lineHeight: "1.5",
})

export const toolDetailTextClass = css({
  color: "fg.muted",
  fontFamily: '"SF Pro Text", "Segoe UI", sans-serif',
  fontSize: "0.84rem",
  fontWeight: "450",
  lineHeight: "1.6",
  letterSpacing: "0.01em",
  whiteSpace: "pre-wrap",
})

export const toolDetailMonoClass = css({
  color: "fg.default",
  fontFamily: '"SF Mono", "Monaco", monospace',
  fontSize: "0.83rem",
  lineHeight: "1.6",
  whiteSpace: "pre-wrap",
})

export const BODY_FONT = '450 15px "SF Pro Text", "Segoe UI", sans-serif'
export const BODY_LINE_HEIGHT = 24
export const META_HEIGHT = 0
export const BUBBLE_PADDING_X = 32
export const BUBBLE_PADDING_Y = 28
export const ROW_GAP = 18
export const VIRTUAL_OVERSCAN_PX = 420
export const MIN_TEXT_WIDTH = 144
export const DEFAULT_ROW_HEIGHT = 104
export const NARROW_BUBBLE_WIDTH_BREAKPOINT = 520
export const WIDE_BUBBLE_WIDTH_BREAKPOINT = 760
export const CONTENT_BLOCK_GAP = 10
export const ATTACHMENT_ROW_HEIGHT = 68
export const TOOL_DIFF_PREVIEW_LINE_LIMIT = 6
