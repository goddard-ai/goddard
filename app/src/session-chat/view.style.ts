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
    minHeight: "100%",
    placeItems: "center",
    padding: "24px",
    color: "muted",
    fontSize: "0.94rem",
    lineHeight: "1.6",
    textAlign: "center",
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
