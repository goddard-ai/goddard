import { css } from "@goddard-ai/styled-system/css"
import { token } from "@goddard-ai/styled-system/tokens"

export default {
  root: css({
    display: "flex",
    flexDirection: "column",
    borderBottom: "1px solid",
    borderColor: "border",
    background: `linear-gradient(180deg, ${token.var("colors.panel")} 0%, ${token.var("colors.surface")} 100%)`,
  }),
  list: css({
    display: "flex",
    alignItems: "stretch",
    gap: "8px",
    padding: "14px 16px 0",
    overflowX: "auto",
  }),
  tab: css({
    display: "inline-flex",
    alignItems: "center",
    gap: "10px",
    minWidth: "0",
    maxWidth: "280px",
    height: "44px",
    paddingInline: "14px",
    border: "1px solid",
    borderColor: "border",
    borderTopLeftRadius: "16px",
    borderTopRightRadius: "16px",
    borderBottomWidth: "0",
    background: `linear-gradient(180deg, ${token.var("colors.surface")} 0%, ${token.var("colors.panel")} 100%)`,
    color: "muted",
    cursor: "pointer",
    transition:
      "background-color 160ms cubic-bezier(0.23, 1, 0.32, 1), color 160ms cubic-bezier(0.23, 1, 0.32, 1), border-color 160ms cubic-bezier(0.23, 1, 0.32, 1), box-shadow 180ms cubic-bezier(0.23, 1, 0.32, 1), transform 160ms cubic-bezier(0.23, 1, 0.32, 1)",
    _active: {
      transform: "scale(0.985)",
    },
    "@media (hover: hover) and (pointer: fine)": {
      _hover: {
        color: "text",
        borderColor: "accent",
      },
    },
    "&[data-active='true']": {
      backgroundColor: "background",
      color: "text",
      borderColor: "accent",
      boxShadow: `0 14px 28px color-mix(in srgb, ${token.var("colors.accent")} 12%, transparent)`,
    },
  }),
  icon: css({
    width: "16px",
    height: "16px",
    flexShrink: "0",
  }),
  label: css({
    overflow: "hidden",
    textOverflow: "ellipsis",
    whiteSpace: "nowrap",
    fontSize: "0.92rem",
    fontWeight: "600",
  }),
  actions: css({
    display: "inline-flex",
    alignItems: "center",
    gap: "8px",
    marginLeft: "auto",
  }),
  dirty: css({
    width: "8px",
    height: "8px",
    borderRadius: "999px",
    backgroundColor: "accentStrong",
    flexShrink: "0",
  }),
  close: css({
    display: "grid",
    placeItems: "center",
    width: "24px",
    height: "24px",
    borderRadius: "999px",
    border: "none",
    backgroundColor: "transparent",
    color: "muted",
    cursor: "pointer",
    transition:
      "background-color 140ms cubic-bezier(0.23, 1, 0.32, 1), color 140ms cubic-bezier(0.23, 1, 0.32, 1), transform 140ms cubic-bezier(0.23, 1, 0.32, 1)",
    _active: {
      transform: "scale(0.94)",
    },
    "@media (hover: hover) and (pointer: fine)": {
      _hover: {
        backgroundColor: "background",
        color: "text",
      },
    },
  }),
}
