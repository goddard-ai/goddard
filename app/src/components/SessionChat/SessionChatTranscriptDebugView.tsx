import { css } from "@goddard-ai/styled-system/css"
import { token } from "@goddard-ai/styled-system/tokens"
import { SESSION_CHAT_TRANSCRIPT_DEBUG_MESSAGES } from "./SessionChatTranscriptDebugData"
import { SessionChatTranscript, type SessionChatTranscriptMessage } from "./SessionChatTranscript"

const debugPageClass = css({
  display: "grid",
  gridTemplateRows: "auto minmax(0, 1fr)",
  gap: "18px",
  height: "100%",
  padding: "24px",
  background:
    `radial-gradient(circle at top right, color-mix(in srgb, ${token.var("colors.accent")} 14%, transparent), transparent 30%), ` +
    `linear-gradient(180deg, ${token.var("colors.background")} 0%, ${token.var("colors.surface")} 100%)`,
})

const debugHeaderClass = css({
  display: "grid",
  gap: "10px",
  padding: "24px 26px",
  borderRadius: "26px",
  border: "1px solid",
  borderColor: "border",
  background: `linear-gradient(180deg, ${token.var("colors.panel")} 0%, ${token.var("colors.background")} 100%)`,
  boxShadow: "0 22px 56px rgba(118, 133, 150, 0.12)",
})

const debugEyebrowClass = css({
  display: "inline-flex",
  alignItems: "center",
  width: "fit-content",
  padding: "7px 11px",
  borderRadius: "999px",
  backgroundColor: "surface",
  color: "accentStrong",
  fontSize: "0.72rem",
  fontWeight: "720",
  letterSpacing: "0.15em",
  textTransform: "uppercase",
})

const debugTitleClass = css({
  color: "text",
  fontSize: "1.6rem",
  fontWeight: "760",
  letterSpacing: "-0.03em",
  lineHeight: "1.08",
})

const debugBodyClass = css({
  maxWidth: "72ch",
  color: "muted",
  fontSize: "0.95rem",
  lineHeight: "1.72",
})

const debugCanvasClass = css({
  minHeight: "0",
  overflow: "hidden",
  borderRadius: "28px",
  border: "1px solid",
  borderColor: "border",
  backgroundColor: "background",
  boxShadow: "0 28px 64px rgba(118, 133, 150, 0.14)",
})

/** Props for the standalone transcript debug view. */
export type SessionChatTranscriptDebugViewProps = {
  messages?: readonly SessionChatTranscriptMessage[]
  scrollCacheKey?: string
}

/** Renders the standalone transcript-debug surface used by the native development menu. */
export function SessionChatTranscriptDebugView(props: SessionChatTranscriptDebugViewProps) {
  const messages = props.messages ?? SESSION_CHAT_TRANSCRIPT_DEBUG_MESSAGES

  return (
    <div class={debugPageClass}>
      <section class={debugHeaderClass}>
        <div class={debugEyebrowClass}>Debug Surface</div>
        <h1 class={debugTitleClass}>SessionChatTranscript</h1>
        <p class={debugBodyClass}>
          This view renders a dumb transcript with injected fixture data, Pretext-based line layout,
          and Virtuoso row virtualization. It is intentionally disconnected from live session state
          so spacing and message rhythm can be tuned in isolation first.
        </p>
      </section>
      <section class={debugCanvasClass}>
        <SessionChatTranscript
          messages={messages}
          scrollCacheKey={props.scrollCacheKey ?? "debug:session-chat-transcript"}
        />
      </section>
    </div>
  )
}

/** Renders the transcript debug surface inside one closable workbench tab. */
export default function SessionChatTranscriptDebugTab(props: { surface: "sessionChatTranscript" }) {
  void props
  return <SessionChatTranscriptDebugView scrollCacheKey="detail:debug:session-chat-transcript" />
}
