import { css } from "@goddard-ai/styled-system/css"

export default {
  root: css({
    display: "inline-flex",
    alignItems: "center",
    gap: "2px",
    minWidth: "0",
    whiteSpace: "nowrap",
  }),
  sequenceSeparator: css({
    marginInline: "4px",
    color: "muted",
    fontSize: "0.82em",
  }),
  symbolKey: css({
    fontFamily: '"SF Pro Text", "-apple-system", "BlinkMacSystemFont", "Segoe UI", sans-serif',
    fontSize: "1.08em",
    lineHeight: "1",
  }),
  characterKey: css({
    fontFamily: '"IBM Plex Mono", "SFMono-Regular", "Menlo", monospace',
    fontSize: "0.94em",
    fontWeight: "720",
    lineHeight: "1",
  }),
  namedKey: css({
    fontFamily: '"SF Pro Text", "-apple-system", "BlinkMacSystemFont", "Segoe UI", sans-serif',
    fontSize: "0.9em",
    fontWeight: "680",
    lineHeight: "1",
  }),
}
