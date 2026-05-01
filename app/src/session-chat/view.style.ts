import { css } from "@goddard-ai/styled-system/css"

export default {
  root: css({
    display: "flex",
    flexDirection: "column",
    minHeight: "100%",
  }),
  transcript: css({
    flex: "1 1 auto",
    minHeight: "0",
  }),
  stateMessage: css({
    display: "grid",
    alignContent: "center",
    justifyItems: "center",
    gap: "10px",
    minHeight: "100%",
    padding: "24px",
    color: "muted",
    textAlign: "center",
  }),
  stateTitle: css({
    color: "text",
    fontSize: "1.02rem",
    fontWeight: "680",
  }),
  stateDescription: css({
    maxWidth: "28rem",
    fontSize: "0.94rem",
    lineHeight: "1.6",
  }),
  stateAction: css({
    marginTop: "4px",
    border: "1px solid token(colors.border)",
    borderRadius: "6px",
    paddingBlock: "7px",
    paddingInline: "12px",
    backgroundColor: "bg.panel",
    color: "text",
    fontSize: "0.9rem",
    fontWeight: "650",
    _hover: {
      backgroundColor: "bg.hover",
    },
  }),
  composerDock: css({
    position: "sticky",
    bottom: "0",
    zIndex: "1",
    paddingInline: "20px",
    paddingBottom: "20px",
    backgroundColor: "background",
  }),
}
