import { css } from "@goddard-ai/styled-system/css"
import { token } from "@goddard-ai/styled-system/tokens"

export default {
  content: css({
    display: "inline-flex",
    alignItems: "center",
    justifyContent: "center",
    pointerEvents: "none",
    padding: "8px 12px",
    borderRadius: "12px",
    border: "1px solid",
    borderColor: "border",
    background: `linear-gradient(180deg, ${token.var("colors.background")} 0%, ${token.var("colors.panel")} 100%)`,
    color: "text",
    fontSize: "0.8rem",
    fontWeight: "620",
    lineHeight: "1",
    boxShadow: "0 18px 40px rgba(121, 138, 160, 0.16)",
    opacity: "1",
    transform: "scale(1)",
    transition:
      "opacity 140ms cubic-bezier(0.23, 1, 0.32, 1), transform 140ms cubic-bezier(0.23, 1, 0.32, 1)",
    zIndex: "10",
    "@starting-style": {
      opacity: "0",
      transform: "scale(0.97)",
    },
  }),
}
