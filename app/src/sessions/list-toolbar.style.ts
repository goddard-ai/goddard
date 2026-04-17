import { css } from "@goddard-ai/styled-system/css"

export default {
  root: css({
    display: "flex",
    alignItems: "flex-start",
    justifyContent: "space-between",
    gap: "16px",
    padding: "20px",
    borderBottom: "1px solid",
    borderColor: "border",
  }),
  content: css({
    display: "grid",
    gap: "6px",
    minWidth: "0",
  }),
  title: css({
    color: "text",
    fontSize: "1.25rem",
    fontWeight: "700",
    lineHeight: "1.25",
  }),
  description: css({
    color: "muted",
    fontSize: "0.9rem",
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
    transition:
      "background-color 160ms cubic-bezier(0.23, 1, 0.32, 1), border-color 160ms cubic-bezier(0.23, 1, 0.32, 1)",
    "@media (hover: hover) and (pointer: fine)": {
      _hover: {
        borderColor: "accentStrong",
        backgroundColor: "background",
      },
    },
  }),
}
