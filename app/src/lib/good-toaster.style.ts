import { css } from "@goddard-ai/styled-system/css"
import { token } from "@goddard-ai/styled-system/tokens"

const accentColor = token.var("colors.accent")
const accentStrongColor = token.var("colors.accentStrong")
const borderColor = token.var("colors.border")
const dangerColor = token.var("colors.danger")
const panelColor = token.var("colors.panel")
const surfaceColor = token.var("colors.surface")

export default {
  group: css({
    width: "min(360px, calc(100vw - 24px))",
    zIndex: "70",
    "@media (max-width: 640px)": {
      width: "calc(100vw - 24px)",
    },
  }),
  root: css({
    "--toast-accent": accentStrongColor,
    display: "grid",
    gridTemplateColumns: "10px minmax(0, 1fr) auto",
    alignItems: "start",
    gap: "12px",
    width: "min(360px, calc(100vw - 24px))",
    padding: "14px",
    borderRadius: "18px",
    border: "1px solid",
    borderColor: "border",
    background: `linear-gradient(180deg, ${surfaceColor} 0%, ${panelColor} 100%)`,
    boxShadow: `0 24px 48px color-mix(in srgb, ${borderColor} 28%, transparent)`,
    translate: "var(--x) var(--y)",
    scale: "var(--scale)",
    zIndex: "var(--z-index)",
    height: "var(--height)",
    opacity: "var(--opacity)",
    willChange: "translate, opacity, scale",
    transition: "translate 400ms, scale 400ms, opacity 400ms, height 400ms, box-shadow 200ms",
    transitionTimingFunction: "cubic-bezier(0.21, 1.02, 0.73, 1)",
    "&[data-state='closed']": {
      transition: "translate 400ms, scale 400ms, opacity 200ms",
      transitionTimingFunction: "cubic-bezier(0.06, 0.71, 0.55, 1)",
    },
    "&[data-type='success']": {
      "--toast-accent": accentStrongColor,
      borderColor: "accent",
      boxShadow: `0 24px 48px color-mix(in srgb, ${accentColor} 18%, transparent)`,
    },
    "&[data-type='error']": {
      "--toast-accent": dangerColor,
      borderColor: "danger",
      boxShadow: `0 24px 48px color-mix(in srgb, ${dangerColor} 18%, transparent)`,
    },
    "@media (max-width: 640px)": {
      width: "calc(100vw - 24px)",
    },
  }),
  accent: css({
    width: "10px",
    minHeight: "42px",
    borderRadius: "999px",
    backgroundColor: "var(--toast-accent)",
    opacity: "0.92",
  }),
  body: css({
    display: "grid",
    gap: "4px",
    minWidth: "0",
    paddingTop: "1px",
  }),
  title: css({
    color: "text",
    fontSize: "13px",
    fontWeight: "700",
    lineHeight: "1.35",
  }),
  description: css({
    color: "muted",
    fontSize: "12px",
    lineHeight: "1.5",
    overflow: "hidden",
    textOverflow: "ellipsis",
    whiteSpace: "nowrap",
  }),
  closeButton: css({
    display: "grid",
    placeItems: "center",
    width: "28px",
    height: "28px",
    marginTop: "-2px",
    border: "none",
    borderRadius: "10px",
    backgroundColor: "transparent",
    color: "muted",
    cursor: "pointer",
    transition:
      "background-color 180ms cubic-bezier(0.23, 1, 0.32, 1), color 180ms cubic-bezier(0.23, 1, 0.32, 1)",
    _hover: {
      backgroundColor: "background",
      color: "text",
    },
    _focusVisible: {
      outline: "2px solid",
      outlineColor: "accentStrong",
      outlineOffset: "2px",
    },
  }),
}
