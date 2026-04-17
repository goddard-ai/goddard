import { css } from "@goddard-ai/styled-system/css"

export default {
  row: css({
    display: "grid",
    gridTemplateColumns: "minmax(0, 1fr) auto",
    gap: "12px",
    padding: "14px 16px",
    borderInlineStart: "2px solid",
  }),
  selectButton: css({
    display: "grid",
    gap: "8px",
    minWidth: "0",
    padding: "0",
    border: "none",
    background: "transparent",
    color: "inherit",
    textAlign: "left",
    cursor: "pointer",
  }),
  metaRow: css({
    display: "flex",
    flexWrap: "wrap",
    alignItems: "center",
    gap: "8px",
  }),
  status: css({
    color: "text",
    fontSize: "0.78rem",
    fontWeight: "600",
    letterSpacing: "0.04em",
    textTransform: "uppercase",
  }),
  updated: css({
    color: "muted",
    fontSize: "0.8rem",
  }),
  content: css({
    display: "grid",
    gap: "8px",
    minWidth: "0",
  }),
  title: css({
    color: "text",
    fontSize: "0.96rem",
    fontWeight: "600",
    lineHeight: "1.4",
  }),
  preview: css({
    color: "muted",
    fontSize: "0.9rem",
    lineHeight: "1.6",
    overflow: "hidden",
    lineClamp: "2",
  }),
  path: css({
    color: "muted",
    fontSize: "0.8rem",
    overflow: "hidden",
    textOverflow: "ellipsis",
    whiteSpace: "nowrap",
  }),
  openButton: css({
    display: "inline-flex",
    alignItems: "center",
    justifyContent: "center",
    gap: "8px",
    alignSelf: "flex-start",
    height: "34px",
    paddingInline: "12px",
    borderRadius: "10px",
    border: "1px solid",
    borderColor: "border",
    backgroundColor: "background",
    color: "text",
    fontSize: "0.84rem",
    fontWeight: "600",
    cursor: "pointer",
    transition:
      "background-color 160ms cubic-bezier(0.23, 1, 0.32, 1), border-color 160ms cubic-bezier(0.23, 1, 0.32, 1)",
    "@media (hover: hover) and (pointer: fine)": {
      _hover: {
        borderColor: "accent",
        backgroundColor: "surface",
      },
    },
  }),
}
