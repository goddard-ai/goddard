import { css } from "@goddard-ai/styled-system/css"

export default {
  scrollPanel: css({
    minHeight: "0",
    height: "100%",
    overflowY: "auto",
    overscrollBehavior: "contain",
  }),
  scrollPanelInner: css({
    height: "100%",
    minHeight: "100%",
  }),
  tabFailureRoot: css({
    display: "grid",
    alignContent: "start",
    gap: "12px",
    minHeight: "100%",
    padding: "20px",
    color: "muted",
  }),
  tabFailureContent: css({
    display: "grid",
    gap: "10px",
    maxWidth: "28rem",
    userSelect: "text",
  }),
  tabFailureTitle: css({
    color: "text",
    fontSize: "1.15rem",
    fontWeight: "720",
    lineHeight: "1.3",
  }),
  tabFailureBody: css({
    fontSize: "0.93rem",
    lineHeight: "1.6",
  }),
  tabFailureAction: css({
    justifySelf: "start",
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
      backgroundColor: "surface",
    },
  }),
}
