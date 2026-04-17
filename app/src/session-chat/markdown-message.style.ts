import { css } from "@goddard-ai/styled-system/css"
import { token } from "@goddard-ai/styled-system/tokens"

export default {
  content: css({
    display: "grid",
    gap: "12px",
    minWidth: 0,
    fontFamily: '"SF Pro Text", "Segoe UI", sans-serif',
    fontSize: "15px",
    fontWeight: "450",
    lineHeight: "24px",
    letterSpacing: "0.01em",
    overflowWrap: "anywhere",
    wordBreak: "break-word",
    "& p": {
      margin: "0",
    },
    "& h1, & h2, & h3, & h4": {
      margin: "0",
      fontFamily: '"Inter Tight", "SF Pro Display", sans-serif',
      fontWeight: "720",
      letterSpacing: "-0.02em",
      lineHeight: "1.15",
      textWrap: "balance",
    },
    "& h1": {
      fontSize: "1.32rem",
    },
    "& h2": {
      fontSize: "1.18rem",
    },
    "& h3, & h4": {
      fontSize: "1.02rem",
    },
    "& ul, & ol": {
      margin: "0",
      paddingInlineStart: "22px",
    },
    "& li + li": {
      marginTop: "6px",
    },
    "& blockquote": {
      margin: "0",
      paddingInlineStart: "14px",
      borderLeft: "3px solid",
      borderLeftColor: "border",
      color: "fg.muted",
    },
    "& hr": {
      border: "0",
      borderTop: "1px solid",
      borderTopColor: "border",
      margin: "2px 0",
    },
    "& a": {
      color: "inherit",
      textDecoration: "underline",
      textUnderlineOffset: "0.18em",
      textDecorationColor: "currentColor",
    },
    "& strong": {
      fontWeight: "720",
    },
    "& em": {
      fontStyle: "italic",
    },
    "& code": {
      fontFamily: '"IBM Plex Mono", "SFMono-Regular", "Menlo", monospace',
      fontSize: "0.92em",
      paddingInline: "0.38em",
      paddingBlock: "0.16em",
      borderRadius: "8px",
      background: "var(--session-chat-inline-code-bg)",
      boxShadow: "inset 0 0 0 1px var(--session-chat-inline-code-border)",
    },
    "& pre": {
      margin: "0",
      maxWidth: "100%",
      overflowX: "auto",
      borderRadius: "16px",
      padding: "14px 16px",
      border: "1px solid var(--session-chat-code-block-border)",
      boxShadow: `0 12px 24px ${token.var("colors.shadow")}`,
    },
    "& pre code": {
      display: "block",
      padding: "0",
      background: "none",
      boxShadow: "none",
      fontSize: "0.88rem",
      lineHeight: "1.6",
    },
    "& table": {
      width: "100%",
      borderCollapse: "collapse",
      fontSize: "0.92em",
    },
    "& th, & td": {
      padding: "8px 10px",
      border: "1px solid",
      borderColor: "border",
      textAlign: "left",
      verticalAlign: "top",
    },
    "& th": {
      fontWeight: "680",
      backgroundColor: "bg.panel",
    },
    "& :where(.shiki)": {
      whiteSpace: "pre",
    },
    "[data-theme=dark] &": {
      "& :where(.shiki)": {
        backgroundColor: "var(--shiki-dark-bg) !important",
        color: "var(--shiki-dark) !important",
      },
      "& :where(.shiki span)": {
        color: "var(--shiki-dark) !important",
      },
    },
  }),
  assistant: css({
    "--session-chat-inline-code-bg": token.var("colors.bg.panel"),
    "--session-chat-inline-code-border": token.var("colors.border"),
    "--session-chat-code-block-border": token.var("colors.border"),
  }),
  user: css({
    "--session-chat-inline-code-bg": token.var("colors.transcript.userCode.bg"),
    "--session-chat-inline-code-border": token.var("colors.transcript.userCode.border"),
    "--session-chat-code-block-border": token.var("colors.transcript.userBubble.border"),
  }),
  caret: css({
    display: "inline-block",
    width: "0.66ch",
    height: "1.05em",
    marginInlineStart: "0.08em",
    verticalAlign: "-0.12em",
    borderRadius: "999px",
    background: "currentColor",
    opacity: "0.72",
  }),
  fallback: css({
    whiteSpace: "pre-wrap",
  }),
}
