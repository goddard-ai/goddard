import { css } from "@goddard-ai/styled-system/css"
import { token } from "@goddard-ai/styled-system/tokens"

export default {
  root: css({
    display: "flex",
    flexDirection: "column",
    gap: "12px",
    width: "92px",
    padding: "18px 16px",
    borderRight: "1px solid",
    borderColor: "border",
    background: `linear-gradient(180deg, ${token.var("colors.surface")} 0%, ${token.var("colors.panel")} 100%)`,
  }),
  brand: css({
    display: "flex",
    flexDirection: "column",
    alignItems: "center",
    gap: "8px",
    marginBottom: "8px",
  }),
  mark: css({
    display: "grid",
    placeItems: "center",
    width: "56px",
    height: "56px",
    borderRadius: "20px",
    background: `linear-gradient(180deg, ${token.var("colors.background")} 0%, ${token.var("colors.panel")} 100%)`,
    boxShadow: `0 16px 32px color-mix(in srgb, ${token.var("colors.accent")} 14%, transparent), inset 0 0 0 1px ${token.var("colors.border")}`,
  }),
  markLabel: css({
    color: "accentStrong",
    fontSize: "0.7rem",
    fontWeight: "700",
    letterSpacing: "0.18em",
    textTransform: "uppercase",
  }),
  item: css({
    position: "relative",
    display: "grid",
    placeItems: "center",
    width: "100%",
    height: "54px",
    borderRadius: "18px",
    border: "none",
    backgroundColor: "transparent",
    color: "muted",
    cursor: "pointer",
    transition:
      "background-color 160ms cubic-bezier(0.23, 1, 0.32, 1), color 160ms cubic-bezier(0.23, 1, 0.32, 1), transform 160ms cubic-bezier(0.23, 1, 0.32, 1), box-shadow 180ms cubic-bezier(0.23, 1, 0.32, 1)",
    _active: {
      transform: "scale(0.97)",
    },
    "@media (hover: hover) and (pointer: fine)": {
      _hover: {
        backgroundColor: "background",
        color: "text",
        boxShadow: `inset 0 0 0 1px ${token.var("colors.border")}`,
      },
    },
    _focusVisible: {
      outline: `2px solid ${token.var("colors.accentStrong")}`,
      outlineOffset: "2px",
    },
    "&[data-selected='true']": {
      background: `linear-gradient(180deg, ${token.var("colors.background")} 0%, ${token.var("colors.panel")} 100%)`,
      color: "accentStrong",
      boxShadow: `0 12px 30px color-mix(in srgb, ${token.var("colors.accent")} 16%, transparent), inset 0 0 0 1px ${token.var("colors.border")}`,
    },
  }),
  icon: css({
    width: "20px",
    height: "20px",
  }),
}
