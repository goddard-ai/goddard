import { css } from "@goddard-ai/styled-system/css"

export default {
  root: css({
    display: "flex",
    flexDirection: "column",
    height: "100%",
  }),
  listBody: css({
    flex: "1",
    minHeight: "0",
    overflowY: "auto",
  }),
}
