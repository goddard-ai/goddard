import type { DaemonSession } from "@goddard-ai/sdk"

import { ListRow } from "./list-row.tsrx"
import styles from "./list.style.ts"

export component SessionsList(props: {
  errorMessage?: string | null;
  listStatus?: "idle" | "loading" | "ready" | "error";
  onOpenChanges: (sessionId: DaemonSession["id"]) => void;
  onOpenSession: (sessionId: DaemonSession["id"]) => void;
  searchQuery: string;
  sessions: readonly DaemonSession[];
}) {
  if (props.listStatus === "loading" && props.sessions.length === 0) {
    <div class={styles.loading}>{"Loading sessions..."}</div>
    return
  }

  if (props.listStatus === "error" && props.sessions.length === 0) {
    <div class={styles.error}>
      <div class={styles.errorContent}>
        <h2 class={styles.title}>{"Couldn't load sessions"}</h2>
        <p class={styles.body}>
          {props.errorMessage ?? "The daemon-backed session list request failed."}
        </p>
      </div>
    </div>
    return
  }

  if (props.sessions.length === 0) {
    <div class={styles.empty}>
      <div class={styles.emptyContent}>
        <h2 class={styles.title}>
          {props.searchQuery.trim().length > 0 ? "No matching sessions" : "No sessions yet"}
        </h2>
        <p class={styles.description}>
          {props.searchQuery.trim().length > 0
            ? "Try a shorter title query or different keywords."
            : "Sessions will appear here once the daemon has created them."}
        </p>
      </div>
    </div>
    return
  }

  <ul class={styles.list}>
    {props.sessions.map(
      (session) => <tsx>
        <li key={session.id}>
          <ListRow onOpen={() => {
            props.onOpenSession(session.id)
          }} onOpenChanges={() => {
            props.onOpenChanges(session.id)
          }} session={session} />
        </li>
      </tsx>,
    )}
  </ul>
}
