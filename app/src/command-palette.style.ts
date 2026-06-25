import { css } from "@goddard-ai/styled-system/css"

export default {
  shortcut: css({
    paddingInline: "6px",
    minWidth: "0",
    height: "20px",
    borderRadius: "6px",
    border: "1px solid {colors.border}",
    backgroundColor: "background",
    color: "muted",
    fontSize: "0.7rem",
    lineHeight: "18px",
  }),
  itemMeta: css({
    minWidth: "0",
    color: "muted",
    fontSize: "0.75rem",
    fontWeight: "500",
  }),
}
