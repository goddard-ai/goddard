import type { DaemonSession } from "@goddard-ai/sdk"
import { Clock3, FolderGit2, Sparkles } from "lucide-react"

import { getSessionDisplayTitle } from "~/sessions/presentation.ts"
import styles from "./header.style.ts"

function formatTimestamp(value: number) {
  return new Intl.DateTimeFormat(undefined, {
    month: "short",
    day: "numeric",
    hour: "numeric",
    minute: "2-digit",
  }).format(new Date(value))
}

export function Header(props: { messageCount: number; session: DaemonSession }) {
  return (
    <header class={styles.root}>
      <div class={styles.content}>
        <div class={styles.badge}>
          <Sparkles size={14} strokeWidth={2} />
          Session chat
        </div>
        <div class={styles.text}>
          <h1 class={styles.title}>{getSessionDisplayTitle(props.session)}</h1>
          <p class={styles.subtitle}>{props.session.lastAgentMessage ?? props.session.cwd}</p>
        </div>
      </div>
      <div class={styles.meta}>
        <span class={styles.metaItem}>
          <FolderGit2 size={14} strokeWidth={2} />
          {props.session.cwd}
        </span>
        <span class={styles.metaItem}>
          <Clock3 size={14} strokeWidth={2} />
          {props.messageCount} messages · {formatTimestamp(props.session.updatedAt)}
        </span>
      </div>
    </header>
  )
}
