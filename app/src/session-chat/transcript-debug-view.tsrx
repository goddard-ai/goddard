import type { SessionTranscriptItem } from "~/sessions/models.ts"
import { SESSION_CHAT_TRANSCRIPT_DEBUG_MESSAGES } from "./transcript-debug-data.ts"
import styles from "./transcript-debug-view.style.ts"
import { Transcript } from "./transcript.tsrx"

/** Props for the standalone transcript debug view. */
export type TranscriptDebugViewProps = {
  messages?: readonly SessionTranscriptItem[];
}

/** Renders the standalone transcript-debug surface used by the native development menu. */
export component TranscriptDebugView(props: TranscriptDebugViewProps) {
  const messages = props.messages ?? SESSION_CHAT_TRANSCRIPT_DEBUG_MESSAGES

  <div class={styles.page}>
    <section class={styles.header}>
      <div class={styles.eyebrow}>{"Debug Surface"}</div>
      <h1 class={styles.title}>{"SessionChatTranscript"}</h1>
      <p class={styles.body}>
        {"This view renders a dumb transcript with injected fixture data, Comark markdown rendering, and Virtuoso row virtualization. It is intentionally disconnected from live session state so spacing and message rhythm can be tuned in isolation first."}
      </p>
    </section>
    <section class={styles.canvas}>
      <Transcript {messages} scrollCacheKey="debug:session-chat-transcript" />
    </section>
  </div>
}

export default TranscriptDebugView
