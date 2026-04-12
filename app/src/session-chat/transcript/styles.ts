import { css } from "@goddard-ai/styled-system/css"
import { token } from "@goddard-ai/styled-system/tokens"

export const transcriptViewportClass = css({
  position: "relative",
  minHeight: "100%",
  paddingInline: "20px",
  paddingBlock: "26px 32px",
  background:
    `radial-gradient(circle at top right, color-mix(in srgb, ${token.var("colors.accent")} 10%, transparent), transparent 26%), ` +
    `linear-gradient(180deg, ${token.var("colors.background")} 0%, ${token.var("colors.surface")} 100%)`,
})

export const loadingStateClass = css({
  position: "absolute",
  inset: "0",
  display: "grid",
  placeItems: "center",
  color: "muted",
  fontSize: "0.94rem",
  letterSpacing: "0.01em",
  pointerEvents: "none",
})

export const rowClass = css({
  paddingInline: "4px",
  paddingBottom: "18px",
})

export const rowInnerClass = css({
  display: "flex",
  width: "100%",
})

export const rowColumnClass = css({
  display: "grid",
  gap: "8px",
})

export const metaRowClass = css({
  display: "flex",
  alignItems: "center",
  gap: "10px",
  color: "muted",
  fontSize: "0.73rem",
  fontWeight: "680",
  letterSpacing: "0.08em",
  textTransform: "uppercase",
})

export const metaAuthorClass = css({
  color: "text",
})

export const metaTimestampClass = css({
  letterSpacing: "0.04em",
})

export const bubbleFrameClass = css({
  display: "inline-flex",
  flexDirection: "column",
  borderRadius: "22px",
  border: "1px solid",
  padding: "14px 16px",
  boxShadow: "0 18px 36px rgba(112, 128, 145, 0.08)",
})

export const assistantBubbleClass = css({
  borderColor: "border",
  background: `linear-gradient(180deg, ${token.var("colors.panel")} 0%, ${token.var("colors.background")} 100%)`,
  color: "text",
})

export const userBubbleClass = css({
  borderColor: "accent",
  background: `linear-gradient(180deg, color-mix(in srgb, ${token.var("colors.accentStrong")} 24%, white), color-mix(in srgb, ${token.var("colors.accent")} 22%, white))`,
  color: "#102030",
})

export const systemBubbleClass = css({
  borderColor: "border",
  background: `linear-gradient(180deg, color-mix(in srgb, ${token.var("colors.surface")} 70%, white), ${token.var("colors.surface")})`,
  color: "muted",
  boxShadow: `inset 0 0 0 1px color-mix(in srgb, ${token.var("colors.border")} 70%, white)`,
})

export const toolBubbleClass = css({
  borderColor: "border",
  background:
    `radial-gradient(circle at top right, color-mix(in srgb, ${token.var("colors.accent")} 8%, transparent), transparent 34%), ` +
    `linear-gradient(180deg, color-mix(in srgb, ${token.var("colors.panel")} 92%, white), ${token.var("colors.background")})`,
  color: "text",
})

export const transcriptLineClass = css({
  fontFamily: '"SF Pro Text", "Segoe UI", sans-serif',
  fontSize: "15px",
  fontWeight: "450",
  lineHeight: "24px",
  letterSpacing: "0.01em",
  whiteSpace: "pre",
})

export const userLineClass = css({
  color: "#102030",
})

export const assistantLineClass = css({
  color: "text",
})

export const systemLineClass = css({
  color: "muted",
})

export const toolCardClass = css({
  display: "grid",
  gap: "14px",
})

export const toolHeaderClass = css({
  display: "flex",
  alignItems: "flex-start",
  justifyContent: "space-between",
  gap: "12px",
  flexWrap: "wrap",
})

export const toolTitleClass = css({
  color: "text",
  fontSize: "0.98rem",
  fontWeight: "700",
  letterSpacing: "-0.01em",
  lineHeight: "1.35",
})

export const toolBadgeRowClass = css({
  display: "flex",
  flexWrap: "wrap",
  gap: "8px",
})

export const toolBadgeClass = css({
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

export const toolKindBadgeClass = css({
  borderColor: "border",
  backgroundColor: "surface",
  color: "muted",
})

export const toolPendingBadgeClass = css({
  borderColor: "border",
  backgroundColor: "surface",
  color: "muted",
})

export const toolRunningBadgeClass = css({
  borderColor: "accent",
  background: `color-mix(in srgb, ${token.var("colors.accent")} 12%, white)`,
  color: "accentStrong",
})

export const toolCompletedBadgeClass = css({
  borderColor: "accentStrong",
  background: `color-mix(in srgb, ${token.var("colors.accentStrong")} 16%, white)`,
  color: "accentStrong",
})

export const toolFailedBadgeClass = css({
  borderColor: "danger",
  background: `color-mix(in srgb, ${token.var("colors.danger")} 14%, white)`,
  color: "danger",
})

export const toolBodyClass = css({
  display: "grid",
  gap: "12px",
})

export const toolSectionClass = css({
  display: "grid",
  gap: "8px",
})

export const toolSectionLabelClass = css({
  color: "muted",
  fontSize: "0.68rem",
  fontWeight: "700",
  letterSpacing: "0.12em",
  textTransform: "uppercase",
})

export const toolTextClass = css({
  color: "text",
  fontFamily: '"SF Pro Text", "Segoe UI", sans-serif',
  fontSize: "0.94rem",
  fontWeight: "450",
  lineHeight: "1.65",
  letterSpacing: "0.01em",
  whiteSpace: "pre-wrap",
})

export const toolDiffClass = css({
  display: "grid",
  gap: "8px",
  padding: "12px 14px",
  borderRadius: "16px",
  border: "1px solid",
  borderColor: "border",
  backgroundColor: "background",
})

export const toolDiffPathClass = css({
  color: "text",
  fontFamily: '"SF Mono", "Monaco", monospace',
  fontSize: "0.83rem",
  fontWeight: "620",
  lineHeight: "1.5",
})

export const toolDiffPreviewClass = css({
  color: "text",
  fontFamily: '"SF Mono", "Monaco", monospace',
  fontSize: "0.82rem",
  lineHeight: "1.6",
  whiteSpace: "pre-wrap",
})

export const toolTerminalClass = css({
  display: "grid",
  gap: "6px",
  padding: "12px 14px",
  borderRadius: "16px",
  border: "1px dashed",
  borderColor: "border",
  backgroundColor: "background",
})

export const toolTerminalLabelClass = css({
  color: "muted",
  fontSize: "0.78rem",
  fontWeight: "700",
  letterSpacing: "0.08em",
  textTransform: "uppercase",
})

export const toolTerminalIdClass = css({
  color: "text",
  fontFamily: '"SF Mono", "Monaco", monospace',
  fontSize: "0.84rem",
  lineHeight: "1.6",
})

export const toolLocationListClass = css({
  display: "grid",
  gap: "8px",
})

export const toolLocationRowClass = css({
  display: "flex",
  flexWrap: "wrap",
  gap: "8px",
  alignItems: "baseline",
})

export const toolLocationPathClass = css({
  color: "text",
  fontFamily: '"SF Mono", "Monaco", monospace',
  fontSize: "0.84rem",
  lineHeight: "1.6",
})

export const toolLocationLineClass = css({
  color: "muted",
  fontSize: "0.82rem",
  lineHeight: "1.6",
})

export const toolEmptyStateClass = css({
  color: "muted",
  fontSize: "0.88rem",
  lineHeight: "1.6",
})

export const BODY_FONT = '450 15px "SF Pro Text", "Segoe UI", sans-serif'
export const BODY_LINE_HEIGHT = 24
export const META_HEIGHT = 32
export const BUBBLE_PADDING_X = 32
export const BUBBLE_PADDING_Y = 28
export const ROW_GAP = 18
export const VIRTUAL_OVERSCAN_PX = 420
export const MIN_TEXT_WIDTH = 144
export const DEFAULT_ROW_HEIGHT = 132
export const NARROW_BUBBLE_WIDTH_BREAKPOINT = 520
export const WIDE_BUBBLE_WIDTH_BREAKPOINT = 760
export const TOOL_DIFF_PREVIEW_LINE_LIMIT = 6
