import { css } from "@goddard-ai/styled-system/css"
import { token } from "@goddard-ai/styled-system/tokens"

const accentColor = token.var("colors.accent")
const accentStrongColor = token.var("colors.accentStrong")
const backgroundColor = token.var("colors.background")
const borderColor = token.var("colors.border")

export default {
  choiceLabel: css({
    position: "relative",
    transition:
      "background-color 160ms cubic-bezier(0.23, 1, 0.32, 1), border-color 160ms cubic-bezier(0.23, 1, 0.32, 1), box-shadow 160ms cubic-bezier(0.23, 1, 0.32, 1)",
    _hover: {
      borderColor: "accent",
      backgroundColor: "surface",
    },
    _focusWithin: {
      borderColor: "accentStrong",
      boxShadow: `0 0 0 3px color-mix(in srgb, ${accentColor} 14%, transparent)`,
    },
    '&[data-selected="true"]': {
      borderColor: "accentStrong",
      backgroundColor: `color-mix(in srgb, ${accentColor} 16%, ${backgroundColor})`,
    },
  }),
  choiceInput: css({
    position: "absolute",
    width: "1px",
    height: "1px",
    overflow: "hidden",
    clip: "rect(0 0 0 0)",
    clipPath: "inset(50%)",
    whiteSpace: "nowrap",
  }),
  choiceIndicator: css({
    display: "inline-flex",
    alignItems: "center",
    justifyContent: "center",
    width: "22px",
    height: "22px",
    border: "1px solid {colors.border}",
    borderRadius: "999px",
    color: "transparent",
    backgroundColor: "panel",
    transition:
      "background-color 160ms cubic-bezier(0.23, 1, 0.32, 1), border-color 160ms cubic-bezier(0.23, 1, 0.32, 1), color 160ms cubic-bezier(0.23, 1, 0.32, 1)",
    '[data-selected="true"] &': {
      borderColor: "accentStrong",
      backgroundColor: "accentStrong",
      color: "accentFg",
    },
  }),
  controlRow: css({
    transition:
      "background-color 160ms cubic-bezier(0.23, 1, 0.32, 1), border-color 160ms cubic-bezier(0.23, 1, 0.32, 1), box-shadow 160ms cubic-bezier(0.23, 1, 0.32, 1)",
    _hover: {
      borderColor: "accent",
      backgroundColor: "surface",
    },
    _focusWithin: {
      borderColor: "accentStrong",
      boxShadow: `0 0 0 3px color-mix(in srgb, ${accentColor} 14%, transparent)`,
    },
    '&[data-selected="true"]': {
      borderColor: "accentStrong",
      backgroundColor: `color-mix(in srgb, ${accentColor} 16%, ${backgroundColor})`,
    },
  }),
  switchInput: css({
    position: "absolute",
    width: "1px",
    height: "1px",
    overflow: "hidden",
    clip: "rect(0 0 0 0)",
    clipPath: "inset(50%)",
    whiteSpace: "nowrap",
  }),
  switchTrack: css({
    position: "relative",
    width: "42px",
    height: "24px",
    border: "1px solid {colors.border}",
    borderRadius: "999px",
    backgroundColor: "panel",
    boxShadow: `inset 0 0 0 1px ${borderColor}`,
    transition:
      "background-color 160ms cubic-bezier(0.23, 1, 0.32, 1), border-color 160ms cubic-bezier(0.23, 1, 0.32, 1)",
    '[data-selected="true"] &': {
      borderColor: "accentStrong",
      backgroundColor: "accentStrong",
      boxShadow: `inset 0 0 0 1px ${accentStrongColor}`,
    },
  }),
  switchThumb: css({
    position: "absolute",
    top: "3px",
    left: "3px",
    width: "16px",
    height: "16px",
    borderRadius: "999px",
    backgroundColor: "background",
    boxShadow: "0 1px 3px rgba(0, 0, 0, 0.18)",
    transition: "transform 160ms cubic-bezier(0.23, 1, 0.32, 1)",
    '[data-selected="true"] &': {
      transform: "translateX(18px)",
      backgroundColor: "accentFg",
    },
  }),
}
