import { css } from "@goddard-ai/styled-system/css"

export default {
  section: css({
    display: "grid",
    gap: "14px",
  }),
  grid: css({
    display: "grid",
    gap: "12px",
    "@media (min-width: 720px)": {
      gridTemplateColumns: "repeat(2, minmax(0, 1fr))",
    },
  }),
  adapterCard: css({
    display: "grid",
    gap: "4px",
    padding: "12px",
    borderRadius: "14px",
    border: "1px solid",
    borderColor: "border",
    backgroundColor: "background",
  }),
}
