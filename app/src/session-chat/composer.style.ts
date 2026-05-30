import { css } from "@goddard-ai/styled-system/css"
import { token } from "@goddard-ai/styled-system/tokens"

export default {
  form: css({
    display: "grid",
    gap: "8px",
  }),
  editorFrame: css({
    position: "relative",
    display: "grid",
    gap: "4px",
    borderRadius: "18px",
    border: "1px solid {colors.border}",
    backgroundColor: "surface",
    boxShadow: "0 16px 36px rgba(98, 112, 128, 0.12)",
    overflow: "hidden",
    transition:
      "border-color 160ms cubic-bezier(0.23, 1, 0.32, 1), box-shadow 160ms cubic-bezier(0.23, 1, 0.32, 1)",
    _focusWithin: {
      borderColor: "accentStrong",
      boxShadow: `0 0 0 3px color-mix(in srgb, ${token.var("colors.accent")} 16%, transparent), 0 16px 36px rgba(98, 112, 128, 0.12)`,
    },
  }),
  contentEditable: css({
    boxSizing: "border-box",
    display: "block",
    width: "100%",
    minHeight: "88px",
    padding: "14px 16px",
    backgroundColor: "transparent",
    color: "text",
    fontSize: "0.94rem",
    lineHeight: "1.6",
    outline: "none",
    whiteSpace: "pre-wrap",
    wordBreak: "break-word",
    "& p": {
      margin: "0",
    },
    "& ul, & ol": {
      margin: "0",
      paddingInlineStart: "22px",
    },
    "& ul": {
      listStyle: "disc",
    },
    "& ol": {
      listStyle: "decimal",
    },
    "& li + li": {
      marginTop: "4px",
    },
  }),
  placeholder: css({
    position: "absolute",
    inset: "14px 16px auto",
    color: "muted",
    fontSize: "0.94rem",
    lineHeight: "1.6",
    pointerEvents: "none",
  }),
  footer: css({
    display: "grid",
    gridTemplateColumns: "minmax(0, 1fr) auto auto",
    alignItems: "center",
    gap: "10px",
    padding: "0 12px 12px",
    "@media (max-width: 719px)": {
      gridTemplateColumns: "minmax(0, 1fr) auto",
    },
  }),
  footerControls: css({
    minWidth: "0",
    "@media (max-width: 719px)": {
      gridColumn: "1 / -1",
    },
  }),
  helperText: css({
    flex: "1 1 auto",
    color: "muted",
    fontSize: "0.83rem",
    lineHeight: "1.6",
  }),
  contextUsage: css({
    position: "relative",
    display: "inline-grid",
    placeItems: "center",
    width: "40px",
    height: "40px",
    flex: "0 0 auto",
    color: "muted",
    fontSize: "0.64rem",
    fontWeight: "700",
    lineHeight: "1",
    "& svg": {
      position: "absolute",
      inset: "0",
      width: "100%",
      height: "100%",
      transform: "rotate(-90deg)",
    },
    "& circle": {
      fill: "none",
      strokeWidth: "3.25",
    },
    "& circle:first-of-type": {
      stroke: "border",
    },
    "& circle:last-of-type": {
      stroke: "accentStrong",
      strokeLinecap: "round",
      transition: "stroke-dasharray 180ms cubic-bezier(0.23, 1, 0.32, 1)",
    },
    "& span": {
      position: "relative",
    },
  }),
  submitButton: css({
    display: "inline-flex",
    alignItems: "center",
    justifyContent: "center",
    width: "40px",
    height: "40px",
    padding: "0",
    borderRadius: "999px",
    border: "1px solid {colors.accent}",
    backgroundColor: "accent",
    color: "text",
    cursor: "pointer",
    _disabled: {
      cursor: "not-allowed",
      opacity: "0.52",
    },
    "& svg": {
      width: "17px",
      height: "17px",
    },
  }),
}
