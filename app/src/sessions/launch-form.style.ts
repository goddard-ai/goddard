import { css } from "@goddard-ai/styled-system/css"
import { token } from "@goddard-ai/styled-system/tokens"

export default {
  form: css({
    display: "grid",
    gap: "12px",
  }),
  editorFrame: css({
    position: "relative",
    borderRadius: "14px",
    border: "1px solid",
    borderColor: "border",
    backgroundColor: "background",
    transition:
      "border-color 160ms cubic-bezier(0.23, 1, 0.32, 1), box-shadow 160ms cubic-bezier(0.23, 1, 0.32, 1)",
    _focusWithin: {
      borderColor: "accentStrong",
      boxShadow: `0 0 0 3px color-mix(in srgb, ${token.var("colors.accent")} 12%, transparent)`,
    },
  }),
  contentEditable: css({
    width: "100%",
    minHeight: "124px",
    padding: "12px 14px",
    color: "text",
    fontSize: "0.9rem",
    lineHeight: "1.55",
    outline: "none",
    whiteSpace: "pre-wrap",
    wordBreak: "break-word",
  }),
  placeholder: css({
    position: "absolute",
    inset: "12px 14px auto",
    color: "muted",
    fontSize: "0.9rem",
    lineHeight: "1.55",
    pointerEvents: "none",
  }),
  footer: css({
    display: "flex",
    alignItems: "center",
    justifyContent: "space-between",
    gap: "12px",
  }),
  helperText: css({
    color: "muted",
    fontSize: "0.82rem",
    lineHeight: "1.55",
  }),
  submitButton: css({
    display: "inline-flex",
    alignItems: "center",
    justifyContent: "center",
    gap: "8px",
    minWidth: "124px",
    height: "40px",
    paddingInline: "14px",
    borderRadius: "12px",
    border: "1px solid",
    borderColor: "accent",
    backgroundColor: "surface",
    color: "text",
    fontSize: "0.88rem",
    fontWeight: "640",
    cursor: "pointer",
    _disabled: {
      cursor: "not-allowed",
      opacity: "0.52",
    },
  }),
  adapterMeta: css({
    color: "text",
    fontSize: "0.9rem",
    fontWeight: "600",
  }),
  mutedMeta: css({
    color: "muted",
    fontWeight: "560",
  }),
  adapterDescription: css({
    color: "muted",
    fontSize: "0.88rem",
    lineHeight: "1.5",
  }),
}
