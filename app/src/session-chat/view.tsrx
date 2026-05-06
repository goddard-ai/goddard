import type { DaemonSession, GetSessionHistoryResponse } from "@goddard-ai/sdk"
import {
  AlertCircle,
  CheckCircle2,
  CircleDot,
  GitBranch,
  LoaderCircle,
  PauseCircle,
  RefreshCcw,
  XCircle,
} from "lucide-react"
import { useEffect, useState } from "preact/hooks"
import { useSigma } from "preact-sigma"
import { isPromise } from "radashi"

import { Composer } from "./composer.tsrx"
import { SessionChat } from "./model.ts"
import type { SessionChatStatus } from "./state.ts"
import { Transcript } from "./transcript.tsrx"
import styles from "./view.style.ts"
import { useWorkbenchTabSet } from "~/app-state-context.tsrx"
import { GoodTooltip } from "~/lib/good-tooltip.tsrx"
import { queryClient, useQueries } from "~/lib/query.ts"
import { useNearestProjectPath, useReportTabProject } from "~/projects/use-tab-project.ts"
import { goddardSdk } from "~/sdk.ts"
import { cancelSessionTurn, reconnectSession, submitSessionPrompt } from "~/sessions/mutations.ts"

export component SessionChatView(props: {
  projectPath: string | null;
  sessionId: DaemonSession["id"];
}) {
  const [, setRetryVersion] = useState(0)

  try {
    const [sessionHistory, loadedSession] = useQueries([
      [goddardSdk.session.history, [{ id: props.sessionId }]],
      [goddardSdk.session.get, [{ id: props.sessionId }]],
    ])

    <LoadedSessionChatView {...props} history={sessionHistory} session={loadedSession.session} />
  } catch (error) {
    if (isPromise(error)) {
      <SessionChatLoadingState />
    } else {
      <main class={styles.root}>
        <SessionChatStateMessage
          actionLabel="Try again"
          description="The daemon-backed session or history request failed."
          onAction={() => {
            retrySessionChatLoad(props.sessionId)
            setRetryVersion((version) => version + 1)
          }}
          title="Couldn't load session"
        />
      </main>
    }
  }
}

component SessionChatLoadingState() {
  <main class={styles.root}>
    <SessionChatStateMessage
      description="Loading the session transcript and current session details."
      title="Loading session"
    />
  </main>
}

component SessionChatStateMessage(props: {
  actionLabel?: string;
  description: string;
  onAction?: () => void;
  title: string;
}) {
  <div class={styles.stateMessage}>
    <h2 class={styles.stateTitle}>{props.title}</h2>
    <p class={styles.stateDescription}>{props.description}</p>
    if (props.actionLabel && props.onAction) {
      <button class={styles.stateAction} type="button" onClick={props.onAction}>
        {props.actionLabel}
      </button>
    }
  </div>
}

/** Retries both reads required for the current query-driven session chat view. */
function retrySessionChatLoad(sessionId: DaemonSession["id"]) {
  queryClient.invalidate(goddardSdk.session.get, [{ id: sessionId }])
  queryClient.invalidate(goddardSdk.session.history, [{ id: sessionId }])
}

function getSessionChatStatusPresentation(status: SessionChatStatus) {
  switch (status) {
    case "running":
      return {
        description: "Agent is working",
        icon: LoaderCircle,
        label: "Running",
        tone: "running",
      }
    case "blocked":
      return {
        description: "Waiting for attention",
        icon: PauseCircle,
        label: "Blocked",
        tone: "blocked",
      }
    case "completed":
      return {
        description: "Session finished",
        icon: CheckCircle2,
        label: "Completed",
        tone: "completed",
      }
    case "failed":
      return {
        description: "Session failed",
        icon: AlertCircle,
        label: "Failed",
        tone: "failed",
      }
    case "cancelled":
      return {
        description: "Session cancelled",
        icon: XCircle,
        label: "Cancelled",
        tone: "cancelled",
      }
    default:
      return {
        description: "Ready for a prompt",
        icon: CircleDot,
        label: "Ready",
        tone: "ready",
      }
  }
}

function formatSessionActionError(error: unknown) {
  if (error instanceof Error && error.message.trim().length > 0) {
    return error.message
  }

  return "The daemon rejected the request."
}

component LoadedSessionChatView(props: {
  history: GetSessionHistoryResponse;
  projectPath: string | null;
  session: DaemonSession;
}) {
  const [submitError, setSubmitError] = useState<unknown>(null)
  const [activeHeaderAction, setActiveHeaderAction] = useState<"cancel" | "reconnect" | null>(null)
  const [headerActionError, setHeaderActionError] = useState<
    {
      description: string;
      title: string;
    } | null
  >(null)
  const [cancelRequestedTurnId, setCancelRequestedTurnId] = useState<string | null>(null)
  const workbenchTabSet = useWorkbenchTabSet()
  const chat = useSigma(
    () => new SessionChat({
      history: props.history,
      session: props.session,
    }),
    { deps: [props.session.id] },
  )
  const resolvedProjectPath = useNearestProjectPath(chat.session.cwd)

  useEffect(() => {
    chat.syncLoadedData(
      {
        history: props.history,
        session: props.session,
      },
    )
  }, [chat, props.history, props.session])

  const status = getSessionChatStatusPresentation(chat.summary.status)
  const StatusIcon = status.icon
  const activeTurnId = chat.summary.activeTurnId
  const cancelAlreadyRequested = activeTurnId !== null && cancelRequestedTurnId === activeTurnId
  const canCancelTurn =
    activeTurnId !== null && chat.session.activeDaemonSession &&
      chat.summary.status === "running" &&
      !cancelAlreadyRequested
  const canReconnect =
    chat.connection.reconnectable && !chat.session.activeDaemonSession &&
      chat.session.connectionMode === "live"
  const canOpenSessionChanges = chat.session.cwd.trim().length > 0

  useReportTabProject(resolvedProjectPath)

  async function reconnectCurrentSession() {
    if (!canReconnect || activeHeaderAction) {
      return
    }

    setActiveHeaderAction("reconnect")
    setHeaderActionError(null)

    try {
      const { session } = await reconnectSession(chat.session.id)
      chat.syncSession(session)
    } catch (error) {
      console.error("Failed to reconnect session.", error)
      setHeaderActionError(
        {
          title: "Reconnect failed",
          description: formatSessionActionError(error),
        },
      )
    } finally {
      setActiveHeaderAction(null)
    }
  }

  async function cancelActiveTurn() {
    if (!canCancelTurn || activeHeaderAction || activeTurnId === null) {
      return
    }

    setActiveHeaderAction("cancel")
    setHeaderActionError(null)

    try {
      const result = await cancelSessionTurn(chat.session.id)

      if (!result.activeTurnCancelled && result.abortedQueue.length === 0) {
        setHeaderActionError(
          {
            title: "Stop skipped",
            description: "The daemon did not find an active turn to stop.",
          },
        )
        return
      }

      setCancelRequestedTurnId(activeTurnId)
    } catch (error) {
      console.error("Failed to stop session turn.", error)
      setHeaderActionError(
        {
          title: "Stop failed",
          description: formatSessionActionError(error),
        },
      )
    } finally {
      setActiveHeaderAction(null)
    }
  }

  function openSessionChanges() {
    if (!canOpenSessionChanges) {
      return
    }

    queryClient.invalidate(goddardSdk.session.changes, [{ id: chat.session.id }])
    workbenchTabSet.openOrFocusTab(
      {
        id: `session-changes:${chat.session.id}`,
        kind: "sessionChanges",
        title: `Changes · ${chat.title}`,
        payload: {
          sessionId: chat.session.id,
          sessionTitle: chat.title,
          repositoryLabel: chat.repositoryLabel,
        },
        dirty: false,
      },
    )
  }

  <main class={styles.root}>
    <header class={styles.header}>
      <div class={styles.headerIdentity}>
        <p class={styles.headerRepository}>
          {chat.repositoryLabel}
        </p>
        <h1 class={styles.headerTitle}>
          {chat.title}
        </h1>
      </div>
      <div class={styles.headerControls}>
        <div
          aria-label={`${status.label}: ${status.description}`}
          class={styles.headerStatus}
          data-tone={status.tone}
          title={status.description}
        >
          <StatusIcon size={15} strokeWidth={2.2} />
          <span class={styles.headerStatusLabel}>
            {status.label}
          </span>
        </div>
        <div class={styles.headerActions}>
          if (canOpenSessionChanges) {
            <GoodTooltip ariaLabel="View changes" content="View changes">
              <button
                aria-label="View session changes"
                class={styles.headerActionButton}
                type="button"
                onClick={openSessionChanges}
              >
                <GitBranch size={15} strokeWidth={2.1} />
              </button>
            </GoodTooltip>
          }
          if (canReconnect) {
            <GoodTooltip ariaLabel="Reconnect session" content="Reconnect session">
              <button
                aria-label="Reconnect session"
                class={styles.headerActionButton}
                disabled={activeHeaderAction !== null}
                type="button"
                onClick={() => {
                  void reconnectCurrentSession()
                }}
              >
                <RefreshCcw size={15} strokeWidth={2.1} />
              </button>
            </GoodTooltip>
          }
          if (canCancelTurn) {
            <GoodTooltip ariaLabel="Stop active turn" content="Stop active turn">
              <button
                aria-label="Stop active turn"
                class={styles.headerActionButton}
                data-tone="danger"
                disabled={activeHeaderAction !== null}
                type="button"
                onClick={() => {
                  void cancelActiveTurn()
                }}
              >
                <XCircle size={15} strokeWidth={2.1} />
              </button>
            </GoodTooltip>
          }
        </div>
      </div>
    </header>
    if (headerActionError) {
      <div class={styles.headerActionError} role="alert">
        <strong class={styles.headerActionErrorTitle}>{headerActionError.title}</strong>
        <span class={styles.headerActionErrorDescription}>
          {headerActionError.description}
        </span>
      </div>
    }
    if (chat.hasEmptyTranscript) {
      <div class={styles.transcript}>
        <SessionChatStateMessage
          description="Send a prompt to start this session."
          title="No messages yet"
        />
      </div>
    } else {
      <Transcript
        class={styles.transcript}
        initialScrollPosition="bottom"
        messages={chat.transcriptMessages}
        scrollCacheKey={`detail:session:${chat.session.id}:transcript`}
      />
    }
    <footer class={styles.composerDock}>
      if (submitError) {
        <div class={styles.composerError} role="alert">
          <strong class={styles.composerErrorTitle}>{"Prompt wasn't sent"}</strong>
          <span class={styles.composerErrorDescription}>
            {"Your draft is still here. Try again when the session is available."}
          </span>
        </div>
      }
      <Composer
        loadSuggestions={async (input) => {
          const response = await goddardSdk.session.composerSuggestions(
            {
              id: chat.session.id,
              trigger: input.trigger,
              query: input.query,
            },
          )

          return response.suggestions
        }}
        onPromptChange={() => {
          if (submitError) {
            setSubmitError(null)
          }
        }}
        onSubmit={async (prompt) => {
          setSubmitError(null)

          try {
            await submitSessionPrompt(
              {
                id: chat.session.id,
                acpId: chat.session.acpSessionId,
                prompt,
              },
            )
          } catch (error) {
            console.error("Failed to submit session prompt.", error)
            setSubmitError(error)
            throw error
          }
        }}
      />
    </footer>
  </main>
}

export default SessionChatView
