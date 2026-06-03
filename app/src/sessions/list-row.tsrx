import type { DaemonSession } from "@goddard-ai/sdk"
import { token } from "@goddard-ai/styled-system/tokens"
import {
  AlertCircle,
  Archive,
  ArchiveRestore,
  CheckCircle2,
  CircleDot,
  FileDiff,
  LoaderCircle,
  PauseCircle,
  XCircle,
} from "lucide-react"
import { useState } from "preact/hooks"

import { GoodTooltip } from "~/lib/good-tooltip.tsrx"
import { useMutations } from "~/lib/mutations-provider.tsx"
import { getSessionDisplayTitle, getSessionRepositoryLabel } from "./display.ts"
import { archiveSession, SessionsPageMutations, unarchiveSession } from "./mutations.ts"
import styles from "./list-row.style.ts"

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
  session: DaemonSession;
}) {
  const [archiveActionPending, setArchiveActionPending] = useState(false)
  const [archiveActionError, setArchiveActionError] = useState<string | null>(null)
  const status = getSessionStatusPresentation(props.session.status)
  const StatusIcon = status.icon
  const sessionTitle = getSessionDisplayTitle(props.session)

  const { openSession, openSessionChanges } = useMutations(SessionsPageMutations)

  <div
    class={styles.row}
    data-session-row="true"
    role="button"
    tabIndex={0}
    onClick={() => openSession(props.session.id)}
    onKeyDown={(event) => {
      handleButtonLikeKeyDown(event, () => openSession(props.session.id))
    }}
  >
    <span class={styles.statusIcon} aria-label={status.label} title={status.label}>
      <StatusIcon
        size={15}
        strokeWidth={2.1}
        style={{
          color: status.color,
        }}
      />
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
      <time
        class={styles.updated}
        data-timestamp="true"
        dateTime={new Date(props.session.updatedAt).toISOString()}
        title={new Intl.DateTimeFormat(undefined, {
          dateStyle: "medium",
          timeStyle: "short",
        }).format(new Date(props.session.updatedAt))}
      >
        {formatTimeLabel(props.session.updatedAt)}
      </time>
      const isArchivedSession = props.session.status === "archived"
      const archiveActionBlocked =
        !isArchivedSession &&
          (props.session.activeDaemonSession || props.session.connectionMode === "live")
      const archiveActionLabel =
        archiveActionError
          ? archiveActionError
          : isArchivedSession
            ? archiveActionPending
              ? "Restoring session"
              : "Unarchive session"
            : archiveActionBlocked
              ? "Archive unavailable while session is live"
              : archiveActionPending
                ? "Archiving session"
                : "Archive session"
      async function toggleSessionArchive(event: preact.TargetedMouseEvent<HTMLButtonElement>) {
        event.stopPropagation()

        if (archiveActionPending || archiveActionBlocked) {
          return
        }

        setArchiveActionPending(true)
        setArchiveActionError(null)

        try {
          if (isArchivedSession) {
            await unarchiveSession(props.session.id)
          } else {
            await archiveSession(props.session.id)
          }
        } catch (error) {
          setArchiveActionError(formatSessionArchiveActionError(error))
        } finally {
          setArchiveActionPending(false)
        }
      }

      <div class={styles.actionGroup} data-row-action="true">
        <GoodTooltip ariaLabel={archiveActionLabel} content={archiveActionLabel}>
          <button
            class={styles.actionButton}
            aria-label={`${archiveActionLabel} for ${sessionTitle}`}
            data-active={isArchivedSession ? "true" : undefined}
            data-tone={archiveActionError ? "danger" : undefined}
            disabled={archiveActionPending || archiveActionBlocked}
            type="button"
            onClick={toggleSessionArchive}
            onKeyDown={(event) => {
              event.stopPropagation()
            }}
          >
            if (isArchivedSession) {
              <ArchiveRestore size={15} strokeWidth={2.1} />
            } else {
              <Archive size={15} strokeWidth={2.1} />
            }
          </button>
        </GoodTooltip>
        <GoodTooltip ariaLabel="View changes" content="View changes">
          <button
            class={styles.actionButton}
            aria-label={`View changes for ${sessionTitle}`}
            type="button"
            onClick={(event) => {
              event.stopPropagation()
              openSessionChanges(props.session.id)
            }}
            onKeyDown={(event) => {
              event.stopPropagation()
            }}
          >
            <FileDiff size={15} strokeWidth={2.1} />
          </button>
        </GoodTooltip>
      </div>
    </div>
  </div>
}

function formatSessionArchiveActionError(error: unknown) {
  if (error instanceof Error && error.message.trim().length > 0) {
    return error.message
  }

  return "Archive action failed"
}
