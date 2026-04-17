import { css } from "@goddard-ai/styled-system/css"
import { token } from "@goddard-ai/styled-system/tokens"

export default {
  chip: css({
    display: "inline-flex",
    alignItems: "center",
    gap: "8px",
    minHeight: "30px",
    paddingInline: "10px",
    borderRadius: "999px",
    border: "1px solid",
    borderColor: "border",
    background: `linear-gradient(180deg, ${token.var("colors.surface")} 0%, ${token.var("colors.background")} 100%)`,
    color: "text",
    boxShadow: `0 10px 20px color-mix(in srgb, ${token.var("colors.accent")} 10%, transparent)`,
  }),
  label: css({
    fontSize: "0.84rem",
    fontWeight: "680",
    lineHeight: "1.2",
  }),
  muted: css({
    color: "muted",
  }),
}
