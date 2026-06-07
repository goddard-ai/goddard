import { css } from "@goddard-ai/styled-system/css"

export default {
  root: css({
    display: "inline-grid",
    placeItems: "center",
    flexShrink: "0",
    color: "inherit",
    lineHeight: "1",
    "& > span": {
      display: "inline-grid",
      placeItems: "center",
      width: "100%",
      height: "100%",
    },
    "& svg": {
      display: "block",
      width: "100%",
      height: "100%",
      color: "inherit",
    },
  }),
  text: css({
    fontSize: "0.95em",
    lineHeight: "1",
  }),
}
