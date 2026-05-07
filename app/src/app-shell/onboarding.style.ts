import { css } from "@goddard-ai/styled-system/css"

export default {
  root: css({
    display: "grid",
    placeItems: "center",
    width: "100%",
    height: "100vh",
    minHeight: "360px",
    padding: "24px",
    backgroundColor: "background",
    color: "text",
    fontFamily: '"Inter Tight", sans-serif',
  }),
  content: css({
    display: "grid",
    justifyItems: "center",
    gap: "18px",
    width: "min(100%, 420px)",
    textAlign: "center",
  }),
  copy: css({
    display: "grid",
    gap: "8px",
  }),
  title: css({
    margin: "0",
    fontSize: "1.45rem",
    fontWeight: "650",
    letterSpacing: "0",
  }),
  description: css({
    margin: "0",
    color: "muted",
    fontSize: "0.94rem",
    lineHeight: "1.45",
  }),
  button: css({
    display: "inline-flex",
    alignItems: "center",
    justifyContent: "center",
    gap: "9px",
    minHeight: "42px",
    paddingInline: "18px",
    border: "1px solid",
    borderColor: "accentStrong",
    borderRadius: "8px",
    backgroundColor: "accentStrong",
    color: "background",
    fontSize: "0.94rem",
    fontWeight: "650",
    cursor: "pointer",
    transition:
      "background-color 160ms cubic-bezier(0.23, 1, 0.32, 1), border-color 160ms cubic-bezier(0.23, 1, 0.32, 1), transform 160ms cubic-bezier(0.23, 1, 0.32, 1)",
    _hover: {
      transform: "translateY(-1px)",
    },
    _focusVisible: {
      outline: "2px solid",
      outlineColor: "accentStrong",
      outlineOffset: "3px",
    },
  }),
}
