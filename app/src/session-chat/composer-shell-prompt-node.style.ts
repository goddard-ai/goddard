import { css } from "@goddard-ai/styled-system/css"
import { token } from "@goddard-ai/styled-system/tokens"

export default {
  prompt: css({
    display: "inline-flex",
    alignItems: "center",
    justifyContent: "center",
    minWidth: "28px",
    minHeight: "28px",
    marginRight: "6px",
    borderRadius: "10px",
    border: "1px solid {colors.border}",
    background: `linear-gradient(180deg, ${token.var("colors.surface")} 0%, ${token.var("colors.background")} 100%)`,
    color: "accentStrong",
    fontFamily: '"IBM Plex Mono", "SFMono-Regular", "Menlo", monospace',
    fontSize: "0.86rem",
    fontWeight: "700",
    lineHeight: "1",
    userSelect: "none",
    boxShadow: `0 10px 20px color-mix(in srgb, ${token.var("colors.accent")} 8%, transparent)`,
  }),
}
