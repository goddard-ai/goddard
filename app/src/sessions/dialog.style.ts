import { css } from "@goddard-ai/styled-system/css"

export default {
  backdrop: css({
    position: "fixed",
    inset: "0",
    backgroundColor: "overlay",
    opacity: "1",
    transition: "opacity 180ms cubic-bezier(0.23, 1, 0.32, 1)",
    zIndex: "60",
    "@starting-style": {
      opacity: "0",
    },
  }),
  positioner: css({
    position: "fixed",
    inset: "0",
    display: "grid",
    justifyItems: "center",
    alignContent: "start",
    padding: "16px",
    zIndex: "61",
    "@media (min-width: 720px)": {
      paddingTop: "40px",
    },
  }),
  content: css({
    width: "min(880px, calc(100vw - 32px))",
    maxHeight: "calc(100vh - 32px)",
    display: "flex",
    flexDirection: "column",
    gap: "14px",
    overflowY: "auto",
    padding: "16px",
    borderRadius: "14px",
    border: "1px solid {colors.border}",
    backgroundColor: "panel",
    opacity: "1",
    transform: "translateY(0)",
    transition:
      "opacity 180ms cubic-bezier(0.23, 1, 0.32, 1), transform 180ms cubic-bezier(0.23, 1, 0.32, 1)",
    outline: "none",
    "@starting-style": {
      opacity: "0",
      transform: "translateY(8px)",
    },
  }),
  header: css({
    display: "grid",
    gap: "4px",
    paddingRight: "44px",
  }),
  title: css({
    margin: "0",
    color: "text",
    fontSize: "1rem",
    fontWeight: "700",
    lineHeight: "1.3",
  }),
  description: css({
    margin: "0",
    color: "muted",
    fontSize: "0.84rem",
    lineHeight: "1.45",
  }),
  closeButton: css({
    position: "absolute",
    top: "12px",
    right: "12px",
    display: "grid",
    placeItems: "center",
    width: "32px",
    height: "32px",
    borderRadius: "8px",
    border: "1px solid {colors.border}",
    backgroundColor: "transparent",
    color: "muted",
    cursor: "pointer",
    _hover: {
      backgroundColor: "surface",
      color: "text",
    },
    _focusVisible: {
      outline: "2px solid",
      outlineColor: "accentStrong",
      outlineOffset: "2px",
    },
  }),
}
