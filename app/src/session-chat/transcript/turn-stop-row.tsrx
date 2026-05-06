import { AlertCircle, CheckCircle2, PauseCircle, XCircle } from "lucide-react"

import type { SessionTranscriptTurnStop } from "~/sessions/models.ts"
import type { MessageListRow } from "../message-list.tsrx"
import {
  rowClass,
  rowInnerClass,
  turnStopIconClass,
  turnStopMetaClass,
  turnStopPillClass,
  turnStopTitleClass,
} from "./transcript.style.ts"

function getTurnStopIcon(status: SessionTranscriptTurnStop["status"]) {
  switch (status) {
    case "completed":
      return CheckCircle2
    case "stopped":
      return PauseCircle
    case "failed":
    case "interrupted":
      return AlertCircle
    default:
      return XCircle
  }
}

function formatTurnStopTimestamp(timestamp: string) {
  const date = new Date(timestamp)

  if (Number.isNaN(date.getTime())) {
    return null
  }

  return new Intl.DateTimeFormat(undefined, {
    hour: "numeric",
    minute: "2-digit",
  }).format(date)
}

/** Renders one compact lifecycle marker after a terminal session turn. */
export component TurnStopTranscriptRow(props: {
  row: MessageListRow<SessionTranscriptTurnStop>;
}) {
  const message = props.row.item
  const StatusIcon = getTurnStopIcon(message.status)
  const timestampLabel =
    message.timestamp ? formatTurnStopTimestamp(message.timestamp) : null

  <article class={rowClass}>
    <div class={rowInnerClass} style={{ justifyContent: "center" }}>
      <div class={turnStopPillClass} data-status={message.status}>
        <span class={turnStopIconClass} aria-hidden="true">
          <StatusIcon size={13} strokeWidth={2.2} />
        </span>
        <span class={turnStopTitleClass}>{message.title}</span>
        if (message.reason) {
          <span class={turnStopMetaClass}>{message.reason}</span>
        }
        if (message.timestamp && timestampLabel) {
          <time class={turnStopMetaClass} dateTime={message.timestamp}>
            {timestampLabel}
          </time>
        }
      </div>
    </div>
  </article>
}
