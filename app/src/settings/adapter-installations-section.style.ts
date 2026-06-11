import { css } from "@goddard-ai/styled-system/css"
import { token } from "@goddard-ai/styled-system/tokens"

const accentColor = token.var("colors.accent")

export default {
  agentGroups: css({
    display: "grid",
    gap: "18px",
  }),
  agentGroup: css({
    display: "grid",
    gap: "8px",
  }),
  agentGroupTitle: css({
    margin: "0",
    color: "text",
    fontSize: "0.86rem",
    fontWeight: "670",
    lineHeight: "1.35",
  }),
  searchField: css({
    display: "flex",
    alignItems: "center",
    gap: "8px",
    width: "min(360px, 100%)",
    height: "34px",
    paddingInline: "10px",
    border: "1px solid {colors.border}",
    borderRadius: "7px",
    backgroundColor: "background",
    color: "muted",
    boxShadow: `0 0 0 3px color-mix(in srgb, ${accentColor} 0%, transparent)`,
    transition:
      "border-color 160ms cubic-bezier(0.23, 1, 0.32, 1), box-shadow 160ms cubic-bezier(0.23, 1, 0.32, 1)",
    _focusWithin: {
      borderColor: "accentStrong",
      boxShadow: `0 0 0 3px color-mix(in srgb, ${accentColor} 14%, transparent)`,
    },
  }),
  searchIcon: css({
    flexShrink: "0",
  }),
  searchInput: css({
    width: "100%",
    minWidth: "0",
    height: "100%",
    padding: "0",
    border: "none",
    outline: "none",
    background: "transparent",
    color: "text",
    fontSize: "0.84rem",
    "&::placeholder": {
      color: "muted",
    },
  }),
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
  adapterLinks: css({
    display: "flex",
    flexWrap: "wrap",
    alignItems: "center",
    gap: "8px",
    paddingBlockStart: "2px",
  }),
  adapterLink: css({
    display: "inline-flex",
    alignItems: "center",
    gap: "5px",
    minWidth: "0",
    color: "accentStrong",
    fontSize: "0.8rem",
    fontWeight: "620",
    lineHeight: "1.4",
    textDecoration: "none",
    _hover: {
      textDecoration: "underline",
      textUnderlineOffset: "2px",
    },
    _focusVisible: {
      outline: "2px solid {colors.accentStrong}",
      outlineOffset: "2px",
    },
  }),
  githubIcon: css({
    width: "14px",
    height: "14px",
    flexShrink: "0",
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
  srOnly: css({
    srOnly: true,
  }),
}
