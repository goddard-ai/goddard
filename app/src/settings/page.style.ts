import { css } from "@goddard-ai/styled-system/css"
import { token } from "@goddard-ai/styled-system/tokens"

const accentColor = token.var("colors.accent")

export default {
  page: css({
    display: "grid",
    gridTemplateColumns: "minmax(0, 760px) 200px",
    alignContent: "start",
    alignItems: "start",
    columnGap: "28px",
    rowGap: "18px",
    minHeight: "100%",
    padding: "24px 28px 32px",
    "@media (max-width: 900px)": {
      gridTemplateColumns: "minmax(0, 1fr)",
      padding: "18px",
    },
  }),
  searchField: css({
    position: "sticky",
    top: "12px",
    zIndex: "2",
    gridColumn: "1 / -1",
    display: "flex",
    alignItems: "center",
    gap: "8px",
    width: "min(420px, 100%)",
    height: "36px",
    paddingInline: "11px",
    border: "1px solid {colors.border}",
    borderRadius: "8px",
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
    fontSize: "0.88rem",
    "&::placeholder": {
      color: "muted",
    },
  }),
  content: css({
    display: "grid",
    gap: "18px",
    minWidth: "0",
  }),
  sidebar: css({
    position: "sticky",
    top: "18px",
    display: "grid",
    gap: "8px",
    paddingBlock: "4px",
    borderLeft: "1px solid {colors.border}",
    backgroundColor: "transparent",
    boxShadow: "none",
    "@media (max-width: 900px)": {
      display: "none",
    },
  }),
  sidebarTitle: css({
    margin: "0",
    color: "muted",
    fontSize: "0.74rem",
    fontWeight: "720",
    lineHeight: "1.3",
    textTransform: "uppercase",
  }),
  sidebarList: css({
    display: "grid",
    gap: "2px",
    listStyle: "none",
    margin: "0",
    padding: "0",
  }),
  sidebarLink: css({
    display: "block",
    minWidth: "0",
    padding: "7px 10px",
    borderRadius: "6px",
    color: "muted",
    fontSize: "0.84rem",
    fontWeight: "600",
    lineHeight: "1.4",
    textDecoration: "none",
    _hover: {
      backgroundColor: "surface",
      color: "text",
    },
    _focusVisible: {
      outline: "2px solid {colors.accent}",
      outlineOffset: "2px",
    },
  }),
  emptyState: css({
    display: "grid",
    gap: "6px",
    maxWidth: "520px",
    padding: "28px",
    border: "1px solid {colors.border}",
    borderRadius: "8px",
    backgroundColor: "panel",
  }),
  emptyTitle: css({
    margin: "0",
    color: "text",
    fontSize: "0.98rem",
    fontWeight: "680",
    lineHeight: "1.35",
  }),
  emptyText: css({
    margin: "0",
    color: "muted",
    fontSize: "0.86rem",
    lineHeight: "1.55",
  }),
  srOnly: css({
    srOnly: true,
  }),
}
