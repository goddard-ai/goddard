import { css } from "@goddard-ai/styled-system/css"

export default {
  transcriptViewport: css({
    position: "relative",
    minHeight: "100%",
    paddingInline: "20px",
    paddingTop: "24px",
    paddingBottom: "32px",
  }),

  loadingState: css({
    position: "absolute",
    inset: "0",
    display: "grid",
    placeItems: "center",
    color: "fg.muted",
    fontSize: "0.94rem",
    letterSpacing: "0.01em",
    pointerEvents: "none",
  }),

  historyPager: css({
    display: "grid",
    justifyItems: "center",
    gap: "8px",
    paddingBottom: "18px",
  }),

  historyLoadButton: css({
    display: "inline-flex",
    alignItems: "center",
    justifyContent: "center",
    gap: "7px",
    minHeight: "30px",
    border: "1px solid token(colors.border)",
    borderRadius: "6px",
    paddingBlock: "5px",
    paddingInline: "11px",
    backgroundColor: "surface",
    color: "fg.default",
    fontSize: "0.82rem",
    fontWeight: "650",
    lineHeight: "1.3",
    _hover: {
      backgroundColor: "bg.hover",
    },
    _disabled: {
      cursor: "default",
      color: "fg.muted",
      opacity: "0.72",
    },
  }),

  historyLoadButtonIcon: css({
    animation: "spin 1s linear infinite",
  }),

  historyLoadError: css({
    display: "inline-flex",
    alignItems: "center",
    gap: "7px",
    maxWidth: "min(100%, 560px)",
    border: "1px solid token(colors.danger)",
    borderRadius: "6px",
    paddingBlock: "6px",
    paddingInline: "10px",
    backgroundColor: "surface",
    color: "danger",
    fontSize: "0.8rem",
    fontWeight: "620",
    lineHeight: "1.4",
  }),

  row: css({
    paddingBottom: "16px",
  }),

  rowInner: css({
    display: "flex",
    width: "100%",
  }),

  rowColumn: css({
    display: "grid",
    gap: "0",
  }),

  bubbleFrame: css({
    display: "inline-flex",
    flexDirection: "column",
    borderRadius: "22px",
    border: "1px solid",
    padding: "14px 16px",
  }),

  assistantMessage: css({
    display: "grid",
    width: "100%",
    color: "fg.default",
  }),

  assistantBubble: css({
    color: "fg.default",
  }),

  userBubble: css({
    borderColor: "transcript.userBubble.border",
    backgroundColor: "transcript.userBubble.end",
    color: "fg.default",
  }),

  transcriptContent: css({
    display: "grid",
    gap: "10px",
    minWidth: 0,
  }),

  attachmentCard: css({
    display: "grid",
    gap: "6px",
    minWidth: 0,
    padding: "2px 0",
  }),

  attachmentHeading: css({
    display: "flex",
    alignItems: "center",
    gap: "8px",
    color: "fg.default",
    fontSize: "0.86rem",
    fontWeight: "680",
    lineHeight: "1.35",
  }),

  attachmentIcon: css({
    display: "inline-flex",
    color: "fg.muted",
  }),

  attachmentDetail: css({
    color: "fg.muted",
    fontSize: "0.8rem",
    lineHeight: "1.45",
    overflowWrap: "anywhere",
    wordBreak: "break-word",
  }),

  toolDisclosure: css({
    display: "grid",
    gap: "10px",
    width: "100%",
  }),

  toolSummary: css({
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
  }),

  toolSummaryText: css({
    color: "fg.default",
    fontSize: "0.9rem",
    fontWeight: "580",
    lineHeight: "1.5",
  }),

  toolInlineMeta: css({
    display: "inline-flex",
    flexWrap: "wrap",
    alignItems: "center",
    gap: "6px",
  }),

  toolInlineSeparator: css({
    display: "inline-flex",
    width: "4px",
    height: "4px",
    flexShrink: 0,
    borderRadius: "999px",
    backgroundColor: "currentColor",
    opacity: "0.55",
  }),

  toolToggleHint: css({
    color: "fg.muted",
    fontSize: "0.82rem",
    lineHeight: "1.5",
    textDecoration: "underline",
    textUnderlineOffset: "0.18em",
  }),

  toolDetailList: css({
    display: "grid",
    gap: "12px",
    margin: "0",
    padding: "0",
    listStyle: "none",
  }),

  toolDetailItem: css({
    display: "grid",
    gap: "6px",
    paddingInlineStart: "12px",
    listStyle: "none",
  }),

  toolDetailHeading: css({
    color: "fg.default",
    fontSize: "0.86rem",
    fontWeight: "620",
    lineHeight: "1.5",
  }),

  toolDetailText: css({
    color: "fg.muted",
    fontFamily: '"SF Pro Text", "Segoe UI", sans-serif',
    fontSize: "0.84rem",
    fontWeight: "450",
    lineHeight: "1.6",
    letterSpacing: "0.01em",
    whiteSpace: "pre-wrap",
  }),

  toolDetailMono: css({
    color: "fg.default",
    fontFamily: '"SF Mono", "Monaco", monospace',
    fontSize: "0.83rem",
    lineHeight: "1.6",
    whiteSpace: "pre-wrap",
  }),

  permissionCard: css({
    display: "grid",
    gap: "12px",
    width: "min(100%, 760px)",
    border: "1px solid token(colors.border)",
    borderRadius: "8px",
    padding: "14px",
    backgroundColor: "surface",
    color: "fg.default",
  }),

  permissionHeader: css({
    display: "grid",
    gridTemplateColumns: "auto minmax(0, 1fr) auto",
    alignItems: "start",
    gap: "10px",
    minWidth: "0",
  }),

  permissionIcon: css({
    display: "inline-flex",
    alignItems: "center",
    justifyContent: "center",
    width: "28px",
    height: "28px",
    border: "1px solid token(colors.border)",
    borderRadius: "6px",
    color: "accentStrong",
  }),

  permissionTitleGroup: css({
    display: "grid",
    gap: "2px",
    minWidth: "0",
  }),

  permissionTitle: css({
    margin: "0",
    color: "fg.default",
    fontSize: "0.92rem",
    fontWeight: "700",
    lineHeight: "1.4",
    overflowWrap: "anywhere",
  }),

  permissionMeta: css({
    color: "fg.muted",
    fontSize: "0.78rem",
    fontWeight: "560",
    lineHeight: "1.45",
  }),

  permissionStatusBadge: css({
    display: "inline-flex",
    alignItems: "center",
    gap: "5px",
    maxWidth: "100%",
    border: "1px solid token(colors.border)",
    borderRadius: "999px",
    paddingBlock: "4px",
    paddingInline: "8px",
    color: "fg.muted",
    fontSize: "0.74rem",
    fontWeight: "700",
    lineHeight: "1.3",
    whiteSpace: "nowrap",
    '&[data-status="pending"]': {
      borderColor: "accent",
      color: "accentStrong",
    },
    '&[data-status="allowed"]': {
      color: "fg.default",
    },
    '&[data-status="denied"]': {
      borderColor: "danger",
      color: "danger",
    },
    '&[data-status="failed"]': {
      borderColor: "danger",
      color: "danger",
    },
  }),

  permissionDetailGrid: css({
    display: "grid",
    gap: "8px",
    minWidth: "0",
  }),

  permissionDetailBlock: css({
    display: "grid",
    gap: "4px",
    minWidth: "0",
  }),

  permissionDetailLabel: css({
    color: "fg.default",
    fontSize: "0.78rem",
    fontWeight: "700",
    lineHeight: "1.4",
  }),

  permissionDetailText: css({
    color: "fg.muted",
    fontFamily: '"SF Pro Text", "Segoe UI", sans-serif',
    fontSize: "0.84rem",
    lineHeight: "1.55",
    overflowWrap: "anywhere",
    whiteSpace: "pre-wrap",
  }),

  permissionDetailMono: css({
    color: "fg.default",
    fontFamily: '"SF Mono", "Monaco", monospace',
    fontSize: "0.8rem",
    lineHeight: "1.55",
    overflowWrap: "anywhere",
    whiteSpace: "pre-wrap",
  }),

  permissionActionRow: css({
    display: "flex",
    flexWrap: "wrap",
    gap: "8px",
    minWidth: "0",
  }),

  permissionOptionButton: css({
    display: "inline-grid",
    gap: "1px",
    minWidth: "112px",
    maxWidth: "100%",
    border: "1px solid token(colors.border)",
    borderRadius: "6px",
    paddingBlock: "7px",
    paddingInline: "10px",
    backgroundColor: "bg.panel",
    color: "fg.default",
    textAlign: "left",
    _hover: {
      backgroundColor: "bg.hover",
    },
    _disabled: {
      cursor: "default",
      opacity: "0.55",
      pointerEvents: "none",
    },
    '&[data-intent="allow"]': {
      borderColor: "accent",
    },
    '&[data-intent="reject"]': {
      color: "danger",
    },
  }),

  permissionOptionName: css({
    minWidth: "0",
    fontSize: "0.84rem",
    fontWeight: "700",
    lineHeight: "1.35",
    overflowWrap: "anywhere",
  }),

  permissionOptionKind: css({
    color: "fg.muted",
    fontSize: "0.72rem",
    fontWeight: "620",
    lineHeight: "1.3",
  }),

  permissionResolution: css({
    color: "fg.muted",
    fontSize: "0.84rem",
    lineHeight: "1.5",
  }),

  permissionError: css({
    border: "1px solid token(colors.danger)",
    borderRadius: "6px",
    paddingBlock: "7px",
    paddingInline: "9px",
    color: "danger",
    fontSize: "0.82rem",
    fontWeight: "620",
    lineHeight: "1.45",
  }),

  planCard: css({
    display: "grid",
    gap: "12px",
    width: "min(100%, 760px)",
    border: "1px solid token(colors.border)",
    borderRadius: "8px",
    padding: "14px",
    backgroundColor: "surface",
    color: "fg.default",
  }),

  planHeader: css({
    display: "grid",
    gridTemplateColumns: "auto minmax(0, 1fr)",
    alignItems: "start",
    gap: "10px",
    minWidth: "0",
  }),

  planIcon: css({
    display: "inline-flex",
    alignItems: "center",
    justifyContent: "center",
    width: "28px",
    height: "28px",
    border: "1px solid token(colors.border)",
    borderRadius: "6px",
    color: "accentStrong",
  }),

  planTitleGroup: css({
    display: "grid",
    gap: "2px",
    minWidth: "0",
  }),

  planTitle: css({
    margin: "0",
    color: "fg.default",
    fontSize: "0.92rem",
    fontWeight: "700",
    lineHeight: "1.4",
    overflowWrap: "anywhere",
  }),

  planMeta: css({
    color: "fg.muted",
    fontSize: "0.78rem",
    fontWeight: "560",
    lineHeight: "1.45",
  }),

  planEntryList: css({
    display: "grid",
    gap: "8px",
    margin: "0",
    padding: "0",
    listStyle: "none",
  }),

  planEntryItem: css({
    display: "grid",
    gridTemplateColumns: "auto minmax(0, 1fr)",
    alignItems: "start",
    gap: "8px",
    minWidth: "0",
  }),

  planEntryStatus: css({
    display: "inline-flex",
    alignItems: "center",
    justifyContent: "center",
    width: "22px",
    height: "22px",
    border: "1px solid token(colors.border)",
    borderRadius: "999px",
    color: "fg.muted",
    '&[data-status="in_progress"]': {
      borderColor: "accent",
      color: "accentStrong",
    },
    '&[data-status="completed"]': {
      color: "fg.default",
    },
  }),

  planEntryContent: css({
    display: "grid",
    gap: "2px",
    minWidth: "0",
  }),

  planEntryText: css({
    color: "fg.default",
    fontSize: "0.86rem",
    fontWeight: "560",
    lineHeight: "1.5",
    overflowWrap: "anywhere",
  }),

  planEntryMeta: css({
    display: "inline-flex",
    flexWrap: "wrap",
    alignItems: "center",
    gap: "6px",
    color: "fg.muted",
    fontSize: "0.74rem",
    fontWeight: "650",
    lineHeight: "1.35",
  }),

  planEmpty: css({
    color: "fg.muted",
    fontSize: "0.84rem",
    lineHeight: "1.5",
  }),

  turnStopPill: css({
    display: "inline-flex",
    flexWrap: "wrap",
    alignItems: "center",
    justifyContent: "center",
    gap: "6px",
    maxWidth: "100%",
    border: "1px solid token(colors.border)",
    borderRadius: "999px",
    paddingBlock: "5px",
    paddingInline: "10px",
    backgroundColor: "surface",
    color: "fg.muted",
    fontSize: "0.8rem",
    fontWeight: "620",
    lineHeight: "1.35",
    '&[data-status="completed"]': {
      color: "fg.muted",
    },
    '&[data-status="stopped"]': {
      borderColor: "accent",
      color: "accentStrong",
    },
    '&[data-status="failed"]': {
      borderColor: "danger",
      color: "danger",
    },
    '&[data-status="cancelled"]': {
      color: "fg.muted",
    },
    '&[data-status="interrupted"]': {
      borderColor: "danger",
      borderStyle: "dashed",
      color: "danger",
    },
  }),

  turnStopIcon: css({
    display: "inline-flex",
    alignItems: "center",
    flexShrink: "0",
  }),

  turnStopTitle: css({
    color: "fg.default",
    fontWeight: "700",
  }),

  turnStopMeta: css({
    minWidth: "0",
    overflow: "hidden",
    textOverflow: "ellipsis",
    whiteSpace: "nowrap",
  }),
}

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
export const TURN_STOP_ROW_HEIGHT = 42
