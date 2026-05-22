import { css } from "@goddard-ai/styled-system/css"

export default {
  spacer: css({
    position: "relative",
    width: "100%",
  }),

  row: css({
    position: "absolute",
    top: "0",
    left: "0",
    width: "100%",
  }),

  loading: css({
    display: "grid",
    minHeight: "100%",
    placeItems: "center",
    color: "muted",
    fontSize: "0.94rem",
    letterSpacing: "0.01em",
    pointerEvents: "none",
  }),
}
