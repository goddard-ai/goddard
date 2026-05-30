import { css } from "@goddard-ai/styled-system/css"

export default {
  failureBackdrop: css({
    position: "fixed",
    inset: "0",
    backgroundColor: "overlay",
  }),
  failurePositioner: css({
    position: "fixed",
    inset: "0",
    display: "grid",
    placeItems: "start center",
    paddingTop: "18vh",
  }),
  failureContent: css({
    display: "grid",
    gap: "14px",
    width: "min(420px, calc(100vw - 32px))",
    padding: "18px",
    border: "1px solid {colors.border}",
    borderRadius: "8px",
    backgroundColor: "surface",
    boxShadow: "0 18px 60px rgba(0, 0, 0, 0.28)",
    color: "text",
  }),
  failureCopy: css({
    display: "grid",
    gap: "7px",
  }),
  failureTitle: css({
    srOnly: true,
  }),
  failureDescription: css({
    color: "muted",
    fontSize: "0.9rem",
    lineHeight: "1.55",
  }),
  failureActions: css({
    display: "flex",
    justifyContent: "flex-end",
    gap: "8px",
  }),
  failureButton: css({
    height: "30px",
    paddingInline: "12px",
    border: "1px solid {colors.border}",
    borderRadius: "7px",
    backgroundColor: "panel",
    color: "text",
    cursor: "pointer",
    fontSize: "0.86rem",
    fontWeight: "640",
    _focusVisible: {
      outline: "2px solid",
      outlineColor: "accentStrong",
      outlineOffset: "2px",
    },
    _hover: {
      backgroundColor: "background",
    },
  }),
}
