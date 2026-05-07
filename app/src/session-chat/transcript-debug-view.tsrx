import { useState } from "preact/hooks"

import {
  createSessionChatQaChat,
  createSessionChatQaPermissionResponse,
  createSessionChatQaStatusSummary,
  findSessionChatQaOlderHistoryPage,
} from "./transcript-debug-data.ts"
import styles from "./transcript-debug-view.style.ts"
import { Transcript } from "./transcript.tsrx"

const qaStatusSummary = createSessionChatQaStatusSummary()

/** Renders the standalone transcript-debug surface used by the native development menu. */
export component TranscriptDebugView() {
  const [chat, setChat] = useState(() => createSessionChatQaChat())
  const [renderVersion, setRenderVersion] = useState(0)
  const [olderHistoryError, setOlderHistoryError] = useState<string | null>(null)
  const canLoadOlderHistory = chat.hasMore && chat.nextCursor !== null

  function refreshTranscript() {
    setRenderVersion((version) => version + 1)
  }

  function resetScenario() {
    setChat(createSessionChatQaChat())
    setOlderHistoryError(null)
    setRenderVersion((version) => version + 1)
  }

  function loadOlderHistory() {
    if (!canLoadOlderHistory) {
      return
    }

    const page = findSessionChatQaOlderHistoryPage(chat.nextCursor)

    if (!page) {
      setOlderHistoryError(`No generated page exists for ${chat.nextCursor}.`)
      return
    }

    chat.prependOlderHistory(page)
    setOlderHistoryError(null)
    refreshTranscript()
  }

  <div class={styles.page}>
    <section class={styles.header}>
      <div class={styles.eyebrow}>{"Debug Surface"}</div>
      <h1 class={styles.title}>{"Session Chat ACP QA"}</h1>
      <p class={styles.body}>
        {"This debug-only surface generates deterministic ACP messages and feeds them through the session chat state model. Use it to inspect transcript rows, permission responses, status summaries, and older-history pagination without starting a real agent."}
      </p>
      <div class={styles.actions}>
        <button class={styles.button} type="button" onClick={loadOlderHistory}>
          {canLoadOlderHistory ? "Load older generated page" : "All generated history loaded"}
        </button>
        <button
          class={styles.button}
          type="button"
          onClick={() => {
            setOlderHistoryError("Generated history request failed.")
          }}
        >
          {"Show paging error"}
        </button>
        <button class={styles.button} type="button" onClick={resetScenario}>
          {"Reset scenario"}
        </button>
      </div>
      <div class={styles.checklist}>
        <span>{"Manual checks"}</span>
        <ol>
          <li>
            {"Scroll to the top, load both older pages, and confirm the current row does not jump."}
          </li>
          <li>{"Click the pending permission option and confirm that row resolves in-place."}</li>
          <li>
            {"Use a real session detail tab to confirm reconnect, stop, and changes header actions."}
          </li>
        </ol>
      </div>
      <div class={styles.statusGrid}>
        for (const item of qaStatusSummary) {
          <article class={styles.statusItem} key={item.label}>
            <strong>{item.label}</strong>
            <span>{item.status}</span>
            <p>{item.description}</p>
            <small>
              {item.actions.length > 0 ? item.actions.join(" · ") : "no header actions"}
            </small>
          </article>
        }
      </div>
    </section>
    <section class={styles.canvas}>
      <Transcript
        messages={chat.transcriptMessages}
        olderHistory={{
          canLoad: canLoadOlderHistory,
          error: olderHistoryError,
          loading: false,
          onLoad: loadOlderHistory,
        }}
        onSelectPermissionOption={async (input) => {
          chat.applyMessage(createSessionChatQaPermissionResponse(input), {
            receivedAt: "2026-04-14T15:12:00.000Z",
          })
          refreshTranscript()
        }}
        scrollCacheKey={`debug:session-chat-acp-qa:${renderVersion}`}
      />
    </section>
  </div>
}

export default TranscriptDebugView
