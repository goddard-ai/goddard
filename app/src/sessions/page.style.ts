import { css } from "@goddard-ai/styled-system/css"

export default {
  root: css({
    display: "grid",
    height: "100%",
    padding: "24px",
  }),
  listPane: css({
    display: "flex",
    flexDirection: "column",
    minHeight: "0",
    height: "100%",
    border: "1px solid",
    borderColor: "border",
    borderRadius: "16px",
    backgroundColor: "panel",
    overflow: "hidden",
  }),
  listBody: css({
    minHeight: "0",
    overflowY: "auto",
  }),
}
