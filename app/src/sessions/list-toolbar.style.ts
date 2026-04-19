import { css } from "@goddard-ai/styled-system/css"

export default {
  root: css({
    display: "flex",
    flexWrap: "wrap",
    alignItems: "center",
    justifyContent: "space-between",
    gap: "12px",
    padding: "16px",
    borderBottom: "1px solid",
    borderColor: "border",
  }),
  content: css({
    display: "grid",
    gap: "4px",
    minWidth: "0",
  }),
  title: css({
    color: "text",
    fontSize: "1.05rem",
    fontWeight: "700",
    lineHeight: "1.25",
  }),
  description: css({
    color: "muted",
    fontSize: "0.82rem",
    lineHeight: "1.4",
  }),
  searchField: css({
    display: "flex",
    alignItems: "center",
    gap: "8px",
    height: "36px",
    width: "min(320px, 100%)",
    paddingInline: "12px",
    borderRadius: "10px",
    border: "1px solid",
    borderColor: "border",
    backgroundColor: "background",
    color: "muted",
    "&:focus-within": {
      borderColor: "accentStrong",
    },
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
