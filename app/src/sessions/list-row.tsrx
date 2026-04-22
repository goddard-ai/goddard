import type { DaemonSession } from "@goddard-ai/sdk"
import { token } from "@goddard-ai/styled-system/tokens"
import {
  AlertCircle,
  CheckCircle2,
  CircleDot,
  GitBranch,
  LoaderCircle,
  PauseCircle,
  XCircle,
} from "lucide-react"

import { GoodTooltip } from "~/lib/good-tooltip.tsrx"
import styles from "./list-row.style.ts"
import { getSessionDisplayTitle, getSessionRepositoryLabel } from "./presentation.ts"

function handleButtonLikeKeyDown(
  event: preact.TargetedKeyboardEvent<HTMLDivElement>,
  onPress: () => void,
) {
  if (event.key !== "Enter" && event.key !== " ") {
    return
  }

  event.preventDefault()
  onPress()
}

function formatTimeLabel(value: number) {
  const diffMinutes = Math.max(0, Math.floor((Date.now() - value) / 60000))

  if (diffMinutes < 1) {
    return "now"
  }

  if (diffMinutes < 60) {
    return `${diffMinutes}m`
  }

  const diffHours = Math.floor(diffMinutes / 60)

  if (diffHours < 24) {
    return `${diffHours}h`
  }

  const diffDays = Math.floor(diffHours / 24)

  if (diffDays < 7) {
    return `${diffDays}d`
  }

  return new Intl.DateTimeFormat(undefined, {
    month: "short",
    day: "numeric",
  }).format(new Date(value))
}

function getSessionStatusPresentation(status: DaemonSession["status"]) {
  switch (status) {
    case "active":
      return {
        icon: LoaderCircle,
        color: token.var("colors.accentStrong"),
        label: "Running",
      }
    case "blocked":
      return {
        icon: PauseCircle,
        color: token.var("colors.text"),
        label: "Blocked",
      }
    case "done":
      return {
        icon: CheckCircle2,
        color: token.var("colors.text"),
        label: "Completed",
      }
    case "error":
      return {
        icon: AlertCircle,
        color: token.var("colors.danger"),
        label: "Error",
      }
    case "cancelled":
      return {
        icon: XCircle,
        color: token.var("colors.muted"),
        label: "Cancelled",
      }
    case "archived":
      return {
        icon: CheckCircle2,
        color: token.var("colors.muted"),
        label: "Archived",
      }
    default:
      return {
        icon: CircleDot,
        color: token.var("colors.muted"),
        label: "Idle",
      }
  }
}

export component ListRow(props: {
  onOpen: () => void;
  onOpenChanges: () => void;
  session: DaemonSession;
}) {
  const status = getSessionStatusPresentation(props.session.status)
  const StatusIcon = status.icon
  const sessionTitle = getSessionDisplayTitle(props.session)

  <tsx:react>
    <div class={styles.row} data-session-row="true" role="button" tabIndex={0} onClick={props.onOpen} onKeyDown={(
      event,
    ) => {
      handleButtonLikeKeyDown(event, props.onOpen)
    }}>
      <span class={styles.statusIcon} aria-label={status.label} title={status.label}>
        <StatusIcon size={15} strokeWidth={2.1} style={{
          color: status.color,
        }} />
        <span class={styles.srOnly}>
          {status.label}
        </span>
      </span>
      <span class={styles.repository} title={props.session.repository?.trim() || props.session.cwd}>
        {getSessionRepositoryLabel(props.session)}
      </span>
      <h2 class={styles.title}>
        {sessionTitle}
      </h2>
      <div class={styles.trailing}>
        <time class={styles.updated} data-timestamp="true" dateTime={new Date(props.session.updatedAt).toISOString()} title={new Intl.DateTimeFormat(undefined, {
          dateStyle: "medium",
          timeStyle: "short",
        }).format(new Date(props.session.updatedAt))}>
          {formatTimeLabel(props.session.updatedAt)}
        </time>
        <GoodTooltip ariaLabel="View changes" content="View changes">
          <button class={styles.actionButton} aria-label={`View changes for ${sessionTitle}`} data-row-action="true" type="button" onClick={(
            event,
          ) => {
            event.stopPropagation()
            props.onOpenChanges()
          }} onKeyDown={(event) => {
            event.stopPropagation()
          }}>
            <GitBranch size={15} strokeWidth={2.1} />
          </button>
        </GoodTooltip>
      </div>
    </div>
  </tsx:react>
}
