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
  composerDock: css({
    position: "sticky",
    bottom: "0",
    zIndex: "1",
    paddingInline: "20px",
    paddingBottom: "20px",
    backgroundColor: "background",
  }),
}
