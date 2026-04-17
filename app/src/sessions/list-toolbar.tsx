import { Plus } from "lucide-react"

import styles from "./list-toolbar.style.ts"

export function ListToolbar(props: { sessionCount: number; onCreateSession: () => void }) {
  return (
    <header class={styles.root}>
      <div class={styles.content}>
        <h1 class={styles.title}>Sessions</h1>
        <p class={styles.description}>
          {props.sessionCount === 0
            ? "Launch the first local session to seed the chat flow."
            : `${props.sessionCount} session${props.sessionCount === 1 ? "" : "s"} available in the workbench.`}
        </p>
      </div>
      <button class={styles.button} type="button" onClick={props.onCreateSession}>
        <Plus size={16} strokeWidth={2.2} />
        New session
      </button>
    </header>
  )
}
