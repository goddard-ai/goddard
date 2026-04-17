import { css } from "@goddard-ai/styled-system/css"

export default {
  root: css({
    minHeight: "100%",
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
