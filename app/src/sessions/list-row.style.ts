import { css } from "@goddard-ai/styled-system/css"

export default {
  row: css({
    display: "flex",
    alignItems: "center",
    gap: "8px",
    minWidth: "0",
    paddingInline: "12px",
    paddingBlock: "8px",
    borderBottom: "1px solid {colors.border}",
    cursor: "pointer",
    borderRadius: "8px",
    outline: "none",
    "@media (hover: hover) and (pointer: fine)": {
      _hover: {
        backgroundColor: "surface",
      },
      '&:hover [data-row-action="true"]': {
        opacity: 1,
        pointerEvents: "auto",
      },
      '&:hover [data-timestamp="true"]': {
        opacity: 0,
      },
    },
    '&:focus-within [data-row-action="true"]': {
      opacity: 1,
      pointerEvents: "auto",
    },
    '&:focus-within [data-timestamp="true"]': {
      opacity: 0,
    },
    _focusVisible: {
      outline: "2px solid",
      outlineColor: "accentStrong",
      outlineOffset: "2px",
    },
  }),
  statusIcon: css({
    display: "inline-flex",
    alignItems: "center",
    justifyContent: "center",
    flexShrink: "0",
  }),
  srOnly: css({
    srOnly: true,
  }),
  repository: css({
    color: "muted",
    fontFamily: '"SF Mono", "JetBrains Mono", "Menlo", monospace',
    fontSize: "0.76rem",
    overflow: "hidden",
    textOverflow: "ellipsis",
    whiteSpace: "nowrap",
    flexShrink: "1",
    maxWidth: "24ch",
  }),
  updated: css({
    color: "muted",
    fontSize: "0.76rem",
    flexShrink: "0",
    transition: "opacity 120ms cubic-bezier(0.23, 1, 0.32, 1)",
  }),
  title: css({
    flex: "1",
    minWidth: "0",
    margin: "0",
    color: "text",
    fontSize: "0.88rem",
    fontWeight: "600",
    lineHeight: "1.3",
    overflow: "hidden",
    textOverflow: "ellipsis",
    whiteSpace: "nowrap",
  }),
  trailing: css({
    position: "relative",
    display: "grid",
    alignItems: "center",
    justifyItems: "end",
    flexShrink: "0",
  }),
  actionGroup: css({
    position: "absolute",
    insetInlineEnd: "0",
    top: "50%",
    transform: "translateY(-50%)",
    display: "inline-flex",
    alignItems: "center",
    gap: "6px",
    opacity: 0,
    pointerEvents: "none",
    transition: "opacity 120ms cubic-bezier(0.23, 1, 0.32, 1)",
  }),
  actionButton: css({
    display: "inline-flex",
    alignItems: "center",
    justifyContent: "center",
    width: "28px",
    height: "28px",
    padding: "0",
    borderRadius: "9px",
    border: "1px solid {colors.border}",
    backgroundColor: "background",
    color: "text",
    transition:
      "background-color 160ms cubic-bezier(0.23, 1, 0.32, 1), border-color 160ms cubic-bezier(0.23, 1, 0.32, 1)",
    _focusVisible: {
      outline: "2px solid",
      outlineColor: "accentStrong",
      outlineOffset: "2px",
    },
    "@media (hover: hover) and (pointer: fine)": {
      _hover: {
        borderColor: "accent",
        backgroundColor: "surface",
      },
    },
    _disabled: {
      cursor: "default",
      opacity: 0.45,
    },
    '&[data-active="true"]': {
      borderColor: "accent",
      backgroundColor: "surface",
      color: "accentStrong",
    },
    '&[data-tone="danger"]': {
      borderColor: "danger",
      color: "danger",
    },
  }),
}
