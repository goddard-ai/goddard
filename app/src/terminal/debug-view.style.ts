import { css } from "@goddard-ai/styled-system/css"
import { token } from "@goddard-ai/styled-system/tokens"

export default {
  page: css({
    display: "grid",
    gridTemplateRows: "auto minmax(0, 1fr)",
    gap: "18px",
    height: "100%",
    padding: "24px",
    background:
      `radial-gradient(circle at top right, color-mix(in srgb, ${token.var("colors.accent")} 14%, transparent), transparent 30%), ` +
      `linear-gradient(180deg, ${token.var("colors.background")} 0%, ${token.var("colors.surface")} 100%)`,
  }),
  header: css({
    display: "grid",
    gap: "14px",
    padding: "24px 26px",
    borderRadius: "26px",
    border: "1px solid",
    borderColor: "border",
    background: `linear-gradient(180deg, ${token.var("colors.panel")} 0%, ${token.var("colors.background")} 100%)`,
    boxShadow: "0 22px 56px rgba(118, 133, 150, 0.12)",
  }),
  eyebrow: css({
    display: "inline-flex",
    alignItems: "center",
    width: "fit-content",
    padding: "7px 11px",
    borderRadius: "999px",
    backgroundColor: "surface",
    color: "accentStrong",
    fontSize: "0.72rem",
    fontWeight: "720",
    letterSpacing: "0.15em",
    textTransform: "uppercase",
  }),
  title: css({
    color: "text",
    fontSize: "1.6rem",
    fontWeight: "760",
    letterSpacing: "-0.03em",
    lineHeight: "1.08",
  }),
  body: css({
    maxWidth: "76ch",
    color: "muted",
    fontSize: "0.95rem",
    lineHeight: "1.72",
  }),
  metaRow: css({
    display: "flex",
    flexWrap: "wrap",
    gap: "10px",
  }),
  metaChip: css({
    display: "inline-flex",
    alignItems: "center",
    padding: "8px 12px",
    borderRadius: "999px",
    border: "1px solid",
    borderColor: "border",
    backgroundColor: "surface",
    color: "text",
    fontFamily: '"IBM Plex Mono", "SFMono-Regular", "Menlo", monospace',
    fontSize: "0.76rem",
  }),
  canvas: css({
    minHeight: "0",
    overflow: "hidden",
    borderRadius: "28px",
    border: "1px solid",
    borderColor: "border",
    backgroundColor: "background",
    boxShadow: "0 28px 64px rgba(118, 133, 150, 0.14)",
  }),
}
