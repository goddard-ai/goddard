import { css } from "@goddard-ai/styled-system/css"

export default {
  root: css({
    display: "grid",
    gridTemplateColumns: "minmax(0, 2fr) minmax(320px, 1fr)",
    gap: "20px",
    height: "100%",
    padding: "24px",
    "@media (max-width: 1040px)": {
      gridTemplateColumns: "1fr",
    },
  }),
  listPane: css({
    display: "flex",
    flexDirection: "column",
    minHeight: "0",
    border: "1px solid",
    borderColor: "border",
    borderRadius: "16px",
    backgroundColor: "panel",
    overflow: "hidden",
  }),
  listBody: css({
    minHeight: "0",
    overflowY: "auto",
  }),
  aside: css({
    display: "grid",
    gap: "16px",
    alignContent: "start",
    padding: "20px",
    border: "1px solid",
    borderColor: "border",
    borderRadius: "16px",
    backgroundColor: "panel",
  }),
  intro: css({
    display: "grid",
    gap: "6px",
  }),
  heading: css({
    color: "text",
    fontSize: "1rem",
    fontWeight: "700",
  }),
  description: css({
    color: "muted",
    fontSize: "0.9rem",
    lineHeight: "1.6",
  }),
  details: css({
    display: "grid",
    gap: "12px",
  }),
  detailCard: css({
    display: "grid",
    gap: "4px",
    padding: "12px",
    borderRadius: "12px",
    border: "1px solid",
    borderColor: "border",
  }),
  detailLabel: css({
    color: "muted",
    fontSize: "0.78rem",
    fontWeight: "600",
  }),
  detailValue: css({
    margin: "0",
    color: "text",
    fontWeight: "600",
  }),
  detailValueWrap: css({
    margin: "0",
    color: "text",
    fontWeight: "600",
    wordBreak: "break-word",
  }),
}
