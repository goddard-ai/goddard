/** Shared dropdown menu styles used by the composer and launch-session selectors. */
import { css } from "@goddard-ai/styled-system/css"
import { token } from "@goddard-ai/styled-system/tokens"

export const inputMenuClass = css({
  zIndex: 70,
  width: "min(380px, var(--available-width))",
  minWidth: "min(280px, var(--available-width))",
  maxWidth: "var(--available-width)",
  maxHeight: "min(360px, var(--available-height))",
  overflow: "hidden",
  border: "1px solid {colors.border}",
  borderRadius: "10px",
  backgroundColor: "panel",
  boxShadow: "0 18px 48px rgba(30, 41, 59, 0.16)",
  outline: "none",
})

export const inputMenuContentClass = css({
  display: "grid",
  gridTemplateRows: "auto minmax(0, 1fr)",
  minHeight: "0",
  maxHeight: "inherit",
})

export const inputMenuHeaderClass = css({
  display: "flex",
  alignItems: "center",
  gap: "8px",
  minHeight: "32px",
  paddingBlock: "8px 4px",
  paddingInline: "10px",
  borderBottom: "1px solid {colors.border}",
  color: "muted",
  fontSize: "0.74rem",
  fontWeight: "680",
  letterSpacing: "0",
})

export const inputMenuFilterClass = css({
  width: "100%",
  height: "38px",
  paddingInline: "12px",
  borderRadius: "12px",
  border: "1px solid {colors.border}",
  backgroundColor: "background",
  color: "text",
  fontSize: "0.88rem",
  outline: "none",
  _focusVisible: {
    borderColor: "accentStrong",
    boxShadow: `0 0 0 3px color-mix(in srgb, ${token.var("colors.accent")} 16%, transparent)`,
  },
})

export const inputMenuListClass = css({
  display: "grid",
  gap: "0",
  minHeight: "0",
  paddingBlock: "4px",
  overflowY: "auto",
})

export const inputMenuButtonClass = css({
  position: "relative",
  display: "grid",
  gridTemplateColumns: "auto minmax(0, 1fr)",
  alignItems: "center",
  gap: "8px",
  width: "100%",
  minHeight: "38px",
  paddingBlock: "7px",
  paddingInline: "10px",
  border: "none",
  backgroundColor: "transparent",
  color: "text",
  cursor: "pointer",
  textAlign: "left",
  outline: "none",
  transition: "background-color 120ms ease",
  _hover: {
    backgroundColor: "surface",
  },
  "&[aria-selected='true']": {
    backgroundColor: "surface",
  },
  _focusVisible: {
    _after: {
      content: '""',
      position: "absolute",
      inset: "2px",
      borderRadius: "8px",
      border: "2px solid {colors.accentStrong}",
      pointerEvents: "none",
    },
  },
  _disabled: {
    cursor: "not-allowed",
    opacity: "0.5",
  },
})

export const inputMenuIconClass = css({
  display: "grid",
  placeItems: "center",
  width: "20px",
  height: "20px",
  color: "muted",
})

export const inputMenuBodyClass = css({
  display: "grid",
  gap: "2px",
  minWidth: "0",
})

export const inputMenuLabelClass = css({
  fontSize: "0.87rem",
  fontWeight: "620",
  lineHeight: "1.35",
  overflow: "hidden",
  textOverflow: "ellipsis",
  whiteSpace: "nowrap",
})

export const inputMenuDetailClass = css({
  minWidth: "0",
  color: "muted",
  fontSize: "0.76rem",
  lineHeight: "1.45",
  overflow: "hidden",
  textOverflow: "ellipsis",
  whiteSpace: "nowrap",
})

export const inputMenuEmptyClass = css({
  display: "grid",
  placeItems: "center",
  minHeight: "80px",
  color: "muted",
  fontSize: "0.84rem",
  textAlign: "center",
  paddingInline: "12px",
})
