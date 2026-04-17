import type { DaemonSession } from "@goddard-ai/sdk"
import { token } from "@goddard-ai/styled-system/tokens"
import { ArrowUpRight } from "lucide-react"

import styles from "./list-row.style.ts"
import { getSessionDisplayTitle, getSessionPreviewText } from "./presentation.ts"

function formatTimeLabel(value: number) {
  return new Intl.DateTimeFormat(undefined, {
    month: "short",
    day: "numeric",
    hour: "numeric",
    minute: "2-digit",
  }).format(new Date(value))
}

export function ListRow(props: {
  isSelected: boolean
  onOpen: () => void
  onSelect: () => void
  session: DaemonSession
}) {
  return (
    <article
      class={styles.row}
      style={{
        borderInlineStartColor: props.isSelected ? token.var("colors.accent") : "transparent",
        backgroundColor: props.isSelected ? token.var("colors.surface") : "transparent",
      }}
    >
      <button class={styles.selectButton} type="button" onClick={props.onSelect}>
        <div class={styles.metaRow}>
          <span class={styles.status}>{props.session.status}</span>
          <span class={styles.updated}>Updated {formatTimeLabel(props.session.updatedAt)}</span>
        </div>
        <div class={styles.content}>
          <h2 class={styles.title}>{getSessionDisplayTitle(props.session)}</h2>
          <p class={styles.preview}>
            {getSessionPreviewText(props.session) || "No transcript yet."}
          </p>
        </div>
        <div class={styles.path}>
          {props.session.lastAgentMessage ? "Transcript ready" : "Waiting for first prompt"}
          {" · "}
          {props.session.cwd}
        </div>
      </button>
      <button class={styles.openButton} type="button" onClick={props.onOpen}>
        Open
        <ArrowUpRight size={15} strokeWidth={2.2} />
      </button>
    </article>
  )
}
