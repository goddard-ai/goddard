import { css } from "@goddard-ai/styled-system/css"

export default {
  backdrop: css({
    position: "fixed",
    inset: "0",
    backgroundColor: "overlay",
    backdropFilter: "blur(6px)",
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
    placeItems: "center",
    padding: "16px",
    zIndex: "61",
  }),
  content: css({
    width: "min(880px, calc(100vw - 32px))",
    maxHeight: "calc(100vh - 32px)",
    display: "flex",
    flexDirection: "column",
    gap: "16px",
    overflowY: "auto",
    padding: "18px",
    borderRadius: "20px",
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
  title: css({
    srOnly: true,
  }),
  closeButton: css({
    position: "absolute",
    top: "12px",
    right: "12px",
    display: "grid",
    placeItems: "center",
    width: "32px",
    height: "32px",
    borderRadius: "10px",
    border: "1px solid {colors.border}",
    backgroundColor: "background",
    color: "muted",
    cursor: "pointer",
  }),
}
