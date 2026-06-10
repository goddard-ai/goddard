import { css } from "@goddard-ai/styled-system/css"
import { token } from "@goddard-ai/styled-system/tokens"

const accentColor = token.var("colors.accent")

export default {
  adapterList: css({
    display: "grid",
    gap: "8px",
    listStyle: "none",
    margin: "0",
    padding: "0",
  }),
  adapterItem: css({
    display: "grid",
    gridTemplateColumns: "minmax(0, 1fr) auto",
    alignItems: "center",
    gap: "12px",
    minHeight: "68px",
    padding: "12px 14px",
    border: "1px solid {colors.border}",
    borderRadius: "8px",
    backgroundColor: "background",
  }),
  adapterBody: css({
    display: "grid",
    gap: "4px",
    minWidth: "0",
  }),
  adapterTitleRow: css({
    display: "flex",
    alignItems: "center",
    gap: "8px",
    minWidth: "0",
  }),
  adapterIcon: css({
    flexShrink: "0",
  }),
  adapterName: css({
    overflow: "hidden",
    color: "text",
    fontSize: "0.9rem",
    fontWeight: "660",
    lineHeight: "1.35",
    textOverflow: "ellipsis",
    whiteSpace: "nowrap",
  }),
  adapterMeta: css({
    color: "muted",
    fontSize: "0.8rem",
    lineHeight: "1.45",
  }),
  adapterDescription: css({
    display: "-webkit-box",
    overflow: "hidden",
    color: "muted",
    fontSize: "0.82rem",
    lineClamp: "2",
    lineHeight: "1.45",
  }),
  actionButton: css({
    display: "inline-flex",
    alignItems: "center",
    justifyContent: "center",
    gap: "7px",
    minWidth: "96px",
    height: "34px",
    paddingInline: "12px",
    border: "1px solid {colors.border}",
    borderRadius: "7px",
    backgroundColor: "panel",
    color: "text",
    fontSize: "0.82rem",
    fontWeight: "650",
    cursor: "pointer",
    transition:
      "background-color 160ms cubic-bezier(0.23, 1, 0.32, 1), border-color 160ms cubic-bezier(0.23, 1, 0.32, 1), box-shadow 160ms cubic-bezier(0.23, 1, 0.32, 1)",
    _hover: {
      borderColor: "accent",
      backgroundColor: "surface",
    },
    _focusVisible: {
      outline: "none",
      borderColor: "accentStrong",
      boxShadow: `0 0 0 3px color-mix(in srgb, ${accentColor} 14%, transparent)`,
    },
    _disabled: {
      opacity: "0.58",
      cursor: "not-allowed",
    },
  }),
  emptyText: css({
    margin: "0",
    color: "muted",
    fontSize: "0.84rem",
    lineHeight: "1.55",
  }),
}
