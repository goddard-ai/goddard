import { css } from "@goddard-ai/styled-system/css"
import { token } from "@goddard-ai/styled-system/tokens"

export default {
  section: css({
    display: "grid",
    gap: "12px",
    transition: "opacity 160ms cubic-bezier(0.23, 1, 0.32, 1)",
    '&[data-disabled="true"]': {
      opacity: "0.62",
    },
  }),
  grid: css({
    display: "grid",
    gap: "10px",
    "@media (min-width: 720px)": {
      gridTemplateColumns: "repeat(2, minmax(0, 1fr))",
    },
  }),
  radioField: css({
    border: "0",
    display: "grid",
    gap: "6px",
    margin: "0",
    minWidth: "0",
    padding: "0",
  }),
  radioLegend: css({
    alignItems: "center",
    color: "muted",
    display: "flex",
    fontSize: "0.78rem",
    gap: "8px",
    lineHeight: "1.2",
  }),
  radioGroup: css({
    border: "1px solid {colors.border}",
    borderRadius: "8px",
    display: "grid",
    gridTemplateColumns: "repeat(2, minmax(0, 1fr))",
    minHeight: "42px",
    overflow: "hidden",
  }),
  radioOption: css({
    alignItems: "center",
    color: "text",
    cursor: "pointer",
    display: "flex",
    fontSize: "0.86rem",
    gap: "8px",
    justifyContent: "space-between",
    lineHeight: "1.2",
    minWidth: "0",
    padding: "0 12px",
    "& + &": {
      borderLeft: "1px solid {colors.border}",
    },
    "&[data-selected='true']": {
      backgroundColor: `color-mix(in srgb, ${token.var("colors.accent")} 16%, transparent)`,
    },
    "& > span": {
      minWidth: "0",
      overflow: "hidden",
      textOverflow: "ellipsis",
      whiteSpace: "nowrap",
    },
    "& > input": {
      flex: "0 0 auto",
    },
  }),
  warning: css({
    margin: "0",
    color: "muted",
    fontSize: "0.82rem",
    lineHeight: "1.45",
  }),
  composerControls: css({
    display: "flex",
    flexWrap: "wrap",
    alignItems: "center",
    justifyContent: "start",
    gap: "12px",
    minWidth: "0",
  }),
  composerWarning: css({
    margin: "2px 0 0",
    color: "muted",
    fontSize: "0.78rem",
    lineHeight: "1.35",
  }),
}
