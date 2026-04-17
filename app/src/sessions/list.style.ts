import { css } from "@goddard-ai/styled-system/css"

export default {
  loading: css({
    display: "grid",
    alignContent: "start",
    minHeight: "320px",
    padding: "20px",
    color: "muted",
  }),
  error: css({
    display: "grid",
    alignContent: "start",
    minHeight: "320px",
    padding: "20px",
    color: "muted",
  }),
  errorContent: css({
    display: "grid",
    gap: "10px",
    maxWidth: "28rem",
  }),
  empty: css({
    display: "grid",
    alignContent: "start",
    gap: "12px",
    minHeight: "220px",
    padding: "20px",
  }),
  emptyContent: css({
    display: "grid",
    gap: "8px",
    maxWidth: "28rem",
  }),
  title: css({
    color: "text",
    fontSize: "1.15rem",
    fontWeight: "720",
  }),
  body: css({
    lineHeight: "1.7",
  }),
  description: css({
    color: "muted",
    fontSize: "0.93rem",
    lineHeight: "1.6",
  }),
  button: css({
    display: "inline-flex",
    alignItems: "center",
    justifyContent: "center",
    gap: "8px",
    height: "36px",
    paddingInline: "12px",
    borderRadius: "10px",
    border: "1px solid",
    borderColor: "accent",
    backgroundColor: "surface",
    color: "text",
    fontSize: "0.86rem",
    fontWeight: "600",
    cursor: "pointer",
  }),
  list: css({
    listStyle: "none",
    margin: "0",
    padding: "0",
    "& > li + li": {
      borderTop: "1px solid",
      borderColor: "border",
    },
  }),
}
