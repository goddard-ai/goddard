import { css } from "@goddard-ai/styled-system/css"
import { token } from "@goddard-ai/styled-system/tokens"

export default {
  page: css({
    display: "flex",
    flexDirection: "column",
    gap: "18px",
    height: "100%",
    padding: "28px",
    background:
      `radial-gradient(circle at top right, color-mix(in srgb, ${token.var("colors.accent")} 14%, transparent), transparent 32%), ` +
      `linear-gradient(180deg, ${token.var("colors.background")} 0%, ${token.var("colors.surface")} 100%)`,
  }),
  card: css({
    maxWidth: "880px",
    padding: "32px",
    borderRadius: "28px",
    border: "1px solid",
    borderColor: "border",
    background: `linear-gradient(180deg, ${token.var("colors.background")} 0%, ${token.var("colors.panel")} 100%)`,
    boxShadow: "0 28px 80px rgba(121, 138, 160, 0.14)",
  }),
  badge: css({
    display: "inline-flex",
    alignItems: "center",
    gap: "8px",
    padding: "8px 12px",
    borderRadius: "999px",
    backgroundColor: "surface",
    marginBottom: "10px",
    color: "accentStrong",
    fontSize: "0.72rem",
    fontWeight: "700",
    letterSpacing: "0.14em",
    textTransform: "uppercase",
  }),
  icon: css({
    width: "14px",
    height: "14px",
  }),
  title: css({
    marginBottom: "12px",
    color: "text",
    fontSize: "1.4rem",
    fontWeight: "700",
  }),
  body: css({
    color: "muted",
    lineHeight: "1.72",
  }),
}
