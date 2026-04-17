import type { DaemonSession } from "@goddard-ai/sdk"
import { Plus } from "lucide-react"

import { ListRow } from "./list-row.tsx"
import styles from "./list.style.ts"

export function SessionsList(props: {
  errorMessage?: string | null
  listStatus?: "idle" | "loading" | "ready" | "error"
  onCreateSession: () => void
  onOpenSession: (sessionId: DaemonSession["id"]) => void
  onSelectSession: (sessionId: DaemonSession["id"]) => void
  selectedSessionId: DaemonSession["id"] | null
  sessions: readonly DaemonSession[]
}) {
  if (props.listStatus === "loading" && props.sessions.length === 0) {
    return <div class={styles.loading}>Loading sessions...</div>
  }

  if (props.listStatus === "error" && props.sessions.length === 0) {
    return (
      <div class={styles.error}>
        <div class={styles.errorContent}>
          <h2 class={styles.title}>Couldn&apos;t load sessions</h2>
          <p class={styles.body}>
            {props.errorMessage ?? "The daemon-backed session list request failed."}
          </p>
        </div>
      </div>
    )
  }

  if (props.sessions.length === 0) {
    return (
      <div class={styles.empty}>
        <div class={styles.emptyContent}>
          <h2 class={styles.title}>No sessions yet</h2>
          <p class={styles.description}>Launch one session to seed the first transcript.</p>
          <div>
            <button class={styles.button} type="button" onClick={props.onCreateSession}>
              <Plus size={16} strokeWidth={2.1} />
              New session
            </button>
          </div>
        </div>
      </div>
    )
  }

  return (
    <ul class={styles.list}>
      {props.sessions.map((session) => (
        <li key={session.id}>
          <ListRow
            isSelected={session.id === props.selectedSessionId}
            onOpen={() => {
              props.onOpenSession(session.id)
            }}
            onSelect={() => {
              props.onSelectSession(session.id)
            }}
            session={session}
          />
        </li>
      ))}
    </ul>
  )
}
