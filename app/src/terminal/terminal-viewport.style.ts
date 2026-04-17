import { css } from "@goddard-ai/styled-system/css"
import { token } from "@goddard-ai/styled-system/tokens"

const viewportPaddingPx = 18

export default {
  frame: css({
    display: "flex",
    minHeight: "280px",
    minWidth: "0",
    borderRadius: "24px",
    border: "1px solid",
    borderColor: "border",
    overflow: "hidden",
    background: `linear-gradient(180deg, ${token.var("colors.background")} 0%, ${token.var("colors.panel")} 100%)`,
    boxShadow: "0 24px 64px rgba(15, 23, 42, 0.14)",
  }),
  viewport: css({
    flex: "1 1 auto",
    minWidth: "0",
    minHeight: "0",
    overflow: "auto",
    padding: `${viewportPaddingPx}px`,
    outline: "none",
    whiteSpace: "pre",
    userSelect: "text",
    cursor: "text",
    scrollbarWidth: "thin",
    scrollbarColor: `${token.var("colors.accentStrong")} transparent`,
    _focusVisible: {
      boxShadow: `inset 0 0 0 2px color-mix(in srgb, ${token.var("colors.accent")} 26%, transparent)`,
    },
  }),
  row: css({
    display: "block",
    minWidth: "fit-content",
  }),
}
