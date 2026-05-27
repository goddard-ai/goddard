import { css } from "@goddard-ai/styled-system/css"

export default {
  page: css({
    display: "grid",
    gridTemplateColumns: "minmax(0, 720px) 220px",
    alignContent: "start",
    alignItems: "start",
    gap: "32px",
    minHeight: "100%",
    padding: "28px",
    "@media (max-width: 900px)": {
      gridTemplateColumns: "minmax(0, 1fr)",
    },
  }),
  content: css({
    display: "grid",
    gap: "24px",
    minWidth: "0",
  }),
  sidebar: css({
    position: "sticky",
    top: "20px",
    display: "grid",
    gap: "10px",
    padding: "14px",
    border: "none",
    borderRadius: "8px",
    backgroundColor: "panel",
    boxShadow: "none",
    "@media (max-width: 900px)": {
      position: "static",
      order: "-1",
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
    padding: "8px 10px",
    borderRadius: "6px",
    color: "text",
    fontSize: "0.88rem",
    fontWeight: "620",
    lineHeight: "1.4",
    textDecoration: "none",
    _hover: {
      backgroundColor: "surface",
    },
    _focusVisible: {
      outline: "2px solid {colors.accent}",
      outlineOffset: "2px",
    },
  }),
}
