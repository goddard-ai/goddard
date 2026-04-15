/** Shared dropdown menu styles used by the composer and launch-session selectors. */
import { css } from "@goddard-ai/styled-system/css"
import { token } from "@goddard-ai/styled-system/tokens"

export const inputMenuClass = css({
  position: "fixed",
  zIndex: 70,
  display: "grid",
  gap: "10px",
  width: "min(360px, calc(100vw - 32px))",
  maxHeight: "min(420px, calc(100vh - 32px))",
  padding: "12px",
  borderRadius: "18px",
  border: "1px solid",
  borderColor: "border",
  background: `linear-gradient(180deg, ${token.var("colors.panel")} 0%, ${token.var("colors.background")} 100%)`,
  boxShadow: "0 24px 60px rgba(98, 112, 128, 0.24)",
})

export const inputMenuHeaderClass = css({
  display: "flex",
  alignItems: "center",
  gap: "8px",
  color: "muted",
  fontSize: "0.76rem",
  fontWeight: "720",
  letterSpacing: "0.08em",
  textTransform: "uppercase",
})

export const inputMenuFilterClass = css({
  width: "100%",
  height: "38px",
  paddingInline: "12px",
  borderRadius: "12px",
  border: "1px solid",
  borderColor: "border",
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
  gap: "6px",
  minHeight: "64px",
  overflowY: "auto",
})

export const inputMenuButtonClass = css({
  display: "grid",
  gridTemplateColumns: "auto minmax(0, 1fr)",
  alignItems: "start",
  gap: "10px",
  width: "100%",
  padding: "10px 12px",
  borderRadius: "14px",
  border: "1px solid transparent",
  backgroundColor: "transparent",
  color: "text",
  cursor: "pointer",
  textAlign: "left",
  transition: "background-color 120ms ease, border-color 120ms ease",
  _disabled: {
    cursor: "not-allowed",
    opacity: "0.5",
  },
})

export const inputMenuButtonActiveClass = css({
  borderColor: "accent",
  background: `linear-gradient(180deg, color-mix(in srgb, ${token.var("colors.accent")} 14%, white), color-mix(in srgb, ${token.var("colors.accent")} 8%, white))`,
})

export const inputMenuIconClass = css({
  display: "grid",
  placeItems: "center",
  width: "28px",
  height: "28px",
  borderRadius: "999px",
  backgroundColor: "surface",
  color: "muted",
})

export const inputMenuBodyClass = css({
  display: "grid",
  gap: "2px",
  minWidth: "0",
})

export const inputMenuLabelClass = css({
  fontSize: "0.87rem",
  fontWeight: "680",
  lineHeight: "1.35",
})

export const inputMenuDetailClass = css({
  color: "muted",
  fontSize: "0.8rem",
  lineHeight: "1.45",
  wordBreak: "break-word",
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
