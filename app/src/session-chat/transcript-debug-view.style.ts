import { css } from "@goddard-ai/styled-system/css"
import { token } from "@goddard-ai/styled-system/tokens"

export default {
  page: css({
    display: "grid",
    gridTemplateRows: "auto minmax(0, 1fr)",
    gap: "18px",
    height: "100%",
    padding: "24px",
  }),
  header: css({
    display: "grid",
    gap: "10px",
    padding: "24px 26px",
    borderRadius: "26px",
    border: "1px solid",
    borderColor: "border",
    background: `linear-gradient(180deg, ${token.var("colors.bg.panel")} 0%, ${token.var("colors.bg.surface")} 100%)`,
    boxShadow: `0 22px 56px ${token.var("colors.shadow")}`,
  }),
  eyebrow: css({
    display: "inline-flex",
    alignItems: "center",
    width: "fit-content",
    padding: "7px 11px",
    borderRadius: "999px",
    backgroundColor: "bg.surface",
    color: "accentStrong",
    fontSize: "0.72rem",
    fontWeight: "720",
    letterSpacing: "0.15em",
    textTransform: "uppercase",
  }),
  title: css({
    color: "fg.default",
    fontSize: "1.6rem",
    fontWeight: "760",
    letterSpacing: "-0.03em",
    lineHeight: "1.08",
  }),
  body: css({
    maxWidth: "72ch",
    color: "fg.muted",
    fontSize: "0.95rem",
    lineHeight: "1.72",
  }),
  canvas: css({
    minHeight: "0",
    overflow: "hidden",
    borderRadius: "28px",
    border: "1px solid",
    borderColor: "border",
    backgroundColor: "bg.canvas",
    boxShadow: `0 28px 64px ${token.var("colors.shadow")}`,
  }),
}
