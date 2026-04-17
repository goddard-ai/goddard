import { css } from "@goddard-ai/styled-system/css"
import { token } from "@goddard-ai/styled-system/tokens"

export default {
  field: css({
    display: "grid",
    gap: "6px",
  }),
  label: css({
    color: "text",
    fontSize: "0.84rem",
    fontWeight: "600",
  }),
  trigger: css({
    display: "grid",
    gridTemplateColumns: "minmax(0, 1fr) auto",
    alignItems: "center",
    gap: "10px",
    width: "100%",
    minHeight: "42px",
    padding: "10px 12px",
    borderRadius: "14px",
    border: "1px solid",
    borderColor: "border",
    backgroundColor: "background",
    color: "text",
    cursor: "pointer",
    textAlign: "left",
    outline: "none",
    transition:
      "border-color 160ms cubic-bezier(0.23, 1, 0.32, 1), box-shadow 160ms cubic-bezier(0.23, 1, 0.32, 1)",
    _focusVisible: {
      borderColor: "accentStrong",
      boxShadow: `0 0 0 3px color-mix(in srgb, ${token.var("colors.accent")} 16%, transparent)`,
    },
    _disabled: {
      cursor: "not-allowed",
      opacity: "0.56",
    },
  }),
  triggerLabel: css({
    minWidth: "0",
    overflow: "hidden",
    textOverflow: "ellipsis",
    whiteSpace: "nowrap",
    fontSize: "0.9rem",
    fontWeight: "600",
  }),
  triggerPlaceholder: css({
    color: "muted",
    fontWeight: "500",
  }),
  chevron: css({
    transition: "transform 160ms cubic-bezier(0.23, 1, 0.32, 1)",
  }),
}
