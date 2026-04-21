import { css } from "@goddard-ai/styled-system/css"

export default {
  searchField: css({
    display: "flex",
    alignItems: "center",
    gap: "8px",
    width: "100%",
    height: "40px",
    paddingInline: "12px",
    borderBottom: "1px solid {colors.border}",
    color: "muted",
  }),
  searchIcon: css({
    flexShrink: "0",
  }),
  searchInput: css({
    width: "100%",
    minWidth: "0",
    height: "100%",
    border: "none",
    background: "transparent",
    color: "text",
    fontSize: "0.84rem",
    outline: "none",
    padding: "0",
    "&::placeholder": {
      color: "muted",
    },
  }),
  srOnly: css({
    position: "absolute",
    width: "1px",
    height: "1px",
    padding: "0",
    margin: "-1px",
    overflow: "hidden",
    clip: "rect(0, 0, 0, 0)",
    whiteSpace: "nowrap",
    borderWidth: "0",
  }),
}
