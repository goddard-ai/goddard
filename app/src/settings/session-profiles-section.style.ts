import { css } from "@goddard-ai/styled-system/css"
import { token } from "@goddard-ai/styled-system/tokens"

const accentColor = token.var("colors.accent")

export default {
  agentSelector: css({
    display: "grid",
    gridTemplateColumns: "minmax(220px, 1fr) minmax(0, 1.5fr)",
    alignItems: "end",
    gap: "12px",
    paddingBlock: "4px 10px",
    "@media (max-width: 660px)": {
      gridTemplateColumns: "minmax(0, 1fr)",
    },
  }),
  projectContext: css({
    minWidth: "0",
    paddingBlock: "9px",
    overflow: "hidden",
    color: "muted",
    fontSize: "0.78rem",
    lineHeight: "1.35",
    textOverflow: "ellipsis",
    whiteSpace: "nowrap",
  }),
  profileList: css({
    display: "grid",
    borderTop: "1px solid {colors.border}",
  }),
  profileRow: css({
    display: "grid",
    gap: "12px",
    paddingBlock: "16px",
    "& + &": {
      borderTop: "1px solid {colors.border}",
    },
  }),
  profileHeader: css({
    display: "flex",
    alignItems: "center",
    justifyContent: "space-between",
    gap: "12px",
  }),
  profileIdentity: css({
    display: "flex",
    alignItems: "baseline",
    flexWrap: "wrap",
    gap: "8px",
    minWidth: "0",
  }),
  profileTitle: css({
    margin: "0",
    color: "text",
    fontSize: "0.9rem",
    fontWeight: "680",
    lineHeight: "1.4",
  }),
  profileStatus: css({
    color: "muted",
    fontSize: "0.76rem",
    lineHeight: "1.4",
    '&[data-tone="warning"]': {
      color: "danger",
    },
  }),
  profileActions: css({
    display: "flex",
    alignItems: "center",
    gap: "7px",
    flexShrink: "0",
  }),
  iconButton: css({
    display: "inline-grid",
    placeItems: "center",
    width: "34px",
    height: "34px",
    padding: "0",
    border: "1px solid {colors.border}",
    borderRadius: "7px",
    backgroundColor: "background",
    color: "muted",
    cursor: "pointer",
    _hover: {
      borderColor: "danger",
      color: "danger",
    },
    _focusVisible: {
      outline: "2px solid {colors.accentStrong}",
      outlineOffset: "2px",
    },
    _disabled: {
      opacity: "0.45",
      cursor: "not-allowed",
    },
  }),
  saveButton: css({
    display: "inline-flex",
    alignItems: "center",
    justifyContent: "center",
    gap: "7px",
    minWidth: "82px",
    height: "34px",
    paddingInline: "12px",
    border: "1px solid {colors.border}",
    borderRadius: "7px",
    backgroundColor: "panel",
    color: "text",
    fontSize: "0.82rem",
    fontWeight: "650",
    cursor: "pointer",
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
      opacity: "0.5",
      cursor: "not-allowed",
    },
  }),
  profileControls: css({
    display: "grid",
    gridTemplateColumns: "repeat(3, minmax(0, 1fr))",
    gap: "10px",
    "@media (max-width: 660px)": {
      gridTemplateColumns: "minmax(0, 1fr)",
    },
  }),
  profileMessage: css({
    margin: "0",
    color: "muted",
    fontSize: "0.8rem",
    lineHeight: "1.5",
  }),
  errorMessage: css({
    margin: "0",
    color: "danger",
    fontSize: "0.82rem",
    lineHeight: "1.5",
  }),
  emptyText: css({
    margin: "0",
    color: "muted",
    fontSize: "0.84rem",
    lineHeight: "1.55",
  }),
}
