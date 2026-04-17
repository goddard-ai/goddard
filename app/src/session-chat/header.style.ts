import { css } from "@goddard-ai/styled-system/css"
import { token } from "@goddard-ai/styled-system/tokens"

export default {
  root: css({
    display: "flex",
    alignItems: "center",
    justifyContent: "space-between",
    gap: "16px",
    padding: "18px 20px",
    borderBottom: "1px solid",
    borderColor: "border",
    background: `linear-gradient(180deg, ${token.var("colors.background")} 0%, ${token.var("colors.surface")} 100%)`,
  }),
  content: css({
    display: "grid",
    gap: "10px",
    minWidth: "0",
  }),
  badge: css({
    display: "inline-flex",
    alignItems: "center",
    gap: "8px",
    width: "fit-content",
    padding: "7px 11px",
    borderRadius: "999px",
    backgroundColor: "surface",
    color: "accentStrong",
    fontSize: "0.72rem",
    fontWeight: "700",
    letterSpacing: "0.12em",
    textTransform: "uppercase",
  }),
  text: css({
    display: "grid",
    gap: "6px",
  }),
  title: css({
    color: "text",
    fontSize: "1.15rem",
    fontWeight: "740",
    letterSpacing: "-0.03em",
    lineHeight: "1.2",
  }),
  subtitle: css({
    color: "muted",
    fontSize: "0.9rem",
    lineHeight: "1.6",
  }),
  meta: css({
    display: "grid",
    gap: "10px",
    justifyItems: "end",
    color: "muted",
    fontSize: "0.8rem",
  }),
  metaItem: css({
    display: "inline-flex",
    alignItems: "center",
    gap: "6px",
  }),
}
