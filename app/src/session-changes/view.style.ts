import { css } from "@goddard-ai/styled-system/css"

export default {
  root: css({
    display: "grid",
    gap: "16px",
    padding: "24px",
  }),
  header: css({
    display: "grid",
    gap: "4px",
  }),
  repository: css({
    color: "muted",
    fontFamily: '"SF Mono", "JetBrains Mono", "Menlo", monospace',
    fontSize: "0.78rem",
  }),
  title: css({
    color: "text",
    fontSize: "1.05rem",
    fontWeight: "700",
    lineHeight: "1.3",
  }),
  description: css({
    color: "muted",
    fontSize: "0.88rem",
    lineHeight: "1.5",
  }),
  workspaceRoot: css({
    color: "muted",
    fontFamily: '"SF Mono", "JetBrains Mono", "Menlo", monospace',
    fontSize: "0.76rem",
    wordBreak: "break-word",
  }),
  panel: css({
    minHeight: "0",
    border: "1px solid",
    borderColor: "border",
    borderRadius: "14px",
    backgroundColor: "panel",
    overflow: "hidden",
  }),
  diffViewport: css({
    overflowX: "auto",
  }),
  diff: css({
    margin: "0",
    padding: "16px",
    color: "text",
    fontFamily: '"SF Mono", "JetBrains Mono", "Menlo", monospace',
    fontSize: "0.8rem",
    lineHeight: "1.6",
    whiteSpace: "pre",
  }),
  empty: css({
    display: "grid",
    gap: "8px",
    padding: "20px",
    border: "1px solid",
    borderColor: "border",
    borderRadius: "14px",
    backgroundColor: "panel",
  }),
  emptyTitle: css({
    color: "text",
    fontSize: "0.96rem",
    fontWeight: "700",
  }),
}
