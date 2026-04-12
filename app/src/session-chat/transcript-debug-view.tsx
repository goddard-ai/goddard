import { css } from "@goddard-ai/styled-system/css"
import { token } from "@goddard-ai/styled-system/tokens"

import type { SessionTranscriptItem } from "~/sessions/models.ts"
import { SESSION_CHAT_TRANSCRIPT_DEBUG_MESSAGES } from "./transcript-debug-data.ts"
import { Transcript } from "./transcript.tsx"

const debugPageClass = css({
  display: "grid",
  gridTemplateRows: "auto minmax(0, 1fr)",
  gap: "18px",
  height: "100%",
  padding: "24px",
})

const debugHeaderClass = css({
  display: "grid",
  gap: "10px",
  padding: "24px 26px",
  borderRadius: "26px",
  border: "1px solid",
  borderColor: "border",
  background: `linear-gradient(180deg, ${token.var("colors.bg.panel")} 0%, ${token.var("colors.bg.surface")} 100%)`,
  boxShadow: `0 22px 56px ${token.var("colors.shadow")}`,
})

const debugEyebrowClass = css({
  display: "inline-flex",
  alignItems: "center",
  width: "fit-content",
  padding: "7px 11px",
  borderRadius: "999px",
  backgroundColor: "bg.surface",
  color: "accentStrong",
  fontSize: "0.72rem",
  fontWeight: "720",
  letterSpacing: "0.15em",
  textTransform: "uppercase",
})

const debugTitleClass = css({
  color: "fg.default",
  fontSize: "1.6rem",
  fontWeight: "760",
  letterSpacing: "-0.03em",
  lineHeight: "1.08",
})

const debugBodyClass = css({
  maxWidth: "72ch",
  color: "fg.muted",
  fontSize: "0.95rem",
  lineHeight: "1.72",
})

const debugCanvasClass = css({
  minHeight: "0",
  overflow: "hidden",
  borderRadius: "28px",
  border: "1px solid",
  borderColor: "border",
  backgroundColor: "bg.canvas",
  boxShadow: `0 28px 64px ${token.var("colors.shadow")}`,
})

/** Props for the standalone transcript debug view. */
export type TranscriptDebugViewProps = {
  messages?: readonly SessionTranscriptItem[]
}

/** Renders the standalone transcript-debug surface used by the native development menu. */
export function TranscriptDebugView(props: TranscriptDebugViewProps) {
  const messages = props.messages ?? SESSION_CHAT_TRANSCRIPT_DEBUG_MESSAGES

  return (
    <div class={debugPageClass}>
      <section class={debugHeaderClass}>
        <div class={debugEyebrowClass}>Debug Surface</div>
        <h1 class={debugTitleClass}>SessionChatTranscript</h1>
        <p class={debugBodyClass}>
          This view renders a dumb transcript with injected fixture data, Comark markdown rendering,
          and Virtuoso row virtualization. It is intentionally disconnected from live session state
          so spacing and message rhythm can be tuned in isolation first.
        </p>
      </section>
      <section class={debugCanvasClass}>
        <Transcript messages={messages} scrollCacheKey="debug:session-chat-transcript" />
      </section>
    </div>
  )
}

export { TranscriptDebugView as default }
