import type { DaemonSession } from "@goddard-ai/sdk"
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
import { useEffect } from "preact/hooks"
import { useSigma } from "preact-sigma"

import { Composer } from "./composer.tsrx"
import { SessionChat } from "./model.ts"
import { SessionChatPageModelProvider, useSessionChatPageModel } from "./page-model.ts"
import { SessionChatQueryDataProvider } from "./query-data.tsrx"
import type { SessionChatStatus } from "./model.ts"
import { Transcript } from "./transcript.tsrx"
import styles from "./view.style.ts"
import { useInbox, useWorkbenchTabSet } from "~/app-state-context.tsrx"
import { GoodTooltip } from "~/lib/good-tooltip.tsrx"
import { queryClient, useQueries } from "~/lib/query.ts"
import { useNearestProjectPath, useReportTabProject } from "~/projects/use-tab-project.ts"
import { goddardSdk } from "~/sdk.ts"
import {
  cancelSessionTurn,
  completeSession,
  reconnectSession,
  respondSessionPermission,
  submitSessionPrompt,
} from "~/sessions/mutations.ts"

export default component SessionChatView(props: {
  projectPath: string | null;
  sessionId: DaemonSession["id"];
}) {
  <SessionChatPageModelProvider>
    const page = useSessionChatPageModel()

    <main class={styles.root}>
      try {
        const [sessionHistory, { session }] = useQueries([
          [goddardSdk.session.history, [{ id: props.sessionId }]],
          [goddardSdk.session.get, [{ id: props.sessionId }]],
        ])
        const chat = useSigma(
          () => new SessionChat({
            history: sessionHistory,
            session,
          }),
          { deps: [session.id] },
        )
        const resolvedProjectPath = useNearestProjectPath(chat.session.cwd)

        useEffect(() => {
          chat.syncLoadedData({
            history: sessionHistory,
            session,
          })
        }, [chat, sessionHistory, session])

        useReportTabProject(resolvedProjectPath)

        const inbox = useInbox()

        useEffect(() => {
          void inbox.markSessionVisited(session.id)
        }, [inbox, session.id])

        const headerAction = page.headerAction
        const activeHeaderAction = headerAction.activeKey.value
        const canReconnect =
          chat.connection.reconnectable && !chat.session.activeDaemonSession &&
            chat.session.connectionMode === "live"

        async function reconnectCurrentSession() {
          if (!canReconnect || activeHeaderAction) {
            return
          }

          await headerAction.run("reconnect", async () => {
            try {
              const { session } = await reconnectSession(chat.session.id)
              chat.syncSession(session)
            } catch (error) {
              console.error("Failed to reconnect session.", error)
              throw {
                title: "Reconnect failed",
                description: formatSessionRequestError(error),
              }
            }
          }).catch(() => {})
        }

        async function completeCurrentSession() {
          if (
            activeHeaderAction !== null || chat.session.activeDaemonSession ||
              chat.session.completedHidden
          ) {
            return
          }

          await headerAction.run("complete", async () => {
            try {
              const result = await completeSession(chat.session.id)

              if (result.item) {
                inbox.applyItem(result.item)
              }

              chat.toggleCompletedHidden(true)
            } catch (error) {
              console.error("Failed to complete session.", error)
              throw {
                title: "Complete failed",
                description: formatSessionRequestError(error),
              }
            }
          }).catch(() => {})
        }

        const cancelRequestedTurnId = page.cancelRequestedTurnId.value
        const activeTurnId = chat.summary.activeTurnId
        const cancelAlreadyRequested =
          activeTurnId !== null && cancelRequestedTurnId === activeTurnId
        const canCancelTurn =
          activeTurnId !== null && chat.session.activeDaemonSession &&
            chat.summary.status === "running" &&
            !cancelAlreadyRequested

        async function cancelActiveTurn() {
          if (!canCancelTurn || activeHeaderAction || activeTurnId === null) {
            return
          }

          await headerAction.run("cancel", async () => {
            try {
              const result = await cancelSessionTurn(chat.session.id)

              if (!result.activeTurnCancelled && result.abortedQueue.length === 0) {
                throw {
                  title: "Stop skipped",
                  description: "The daemon did not find an active turn to stop.",
                }
              }

              page.markCancelRequested(activeTurnId)
            } catch (error) {
              if (isSessionChatHeaderActionError(error)) {
                throw error
              }

              console.error("Failed to stop session turn.", error)
              throw {
                title: "Stop failed",
                description: formatSessionRequestError(error),
              }
            }
          }).catch(() => {})
        }

        const workbenchTabSet = useWorkbenchTabSet()
        const canOpenSessionChanges = chat.session.cwd.trim().length > 0

        function openSessionChanges() {
          if (!canOpenSessionChanges) {
            return
          }

          queryClient.invalidate(goddardSdk.session.changes, [{ id: chat.session.id }])
          workbenchTabSet.openOrFocusTab({
            id: `session-changes:${chat.session.id}`,
            kind: "sessionChanges",
            title: `Changes · ${chat.title}`,
            payload: {
              sessionId: chat.session.id,
              sessionTitle: chat.title,
              repositoryLabel: chat.repositoryLabel,
            },
            dirty: false,
          })
        }

        const olderHistory = page.olderHistory
        const canLoadOlderHistory = chat.hasMore && chat.nextCursor !== null

        async function loadOlderHistory() {
          if (!canLoadOlderHistory || olderHistory.isPending.value) {
            return
          }

          try {
            await olderHistory.run(() => chat.loadOlderHistory())
          } catch (error) {
            console.error("Failed to load older session history.", error)
          }
        }

        <SessionChatQueryDataProvider value={{ chat, session, sessionHistory }}>
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
              const status = getSessionChatStatusPresentation(chat.summary.status)
              const StatusIcon = status.icon

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
                const completeActionDisabled =
                  activeHeaderAction !== null || chat.session.activeDaemonSession ||
                    chat.session.completedHidden
                const completeActionLabel =
                  chat.session.completedHidden
                    ? "Session completed"
                    : chat.session.activeDaemonSession
                      ? "Complete unavailable while agent is active"
                      : activeHeaderAction === "complete"
                        ? "Completing session"
                        : "Complete session"

                <GoodTooltip ariaLabel={completeActionLabel} content={completeActionLabel}>
                  <button
                    aria-label={completeActionLabel}
                    class={styles.headerActionButton}
                    data-active={chat.session.completedHidden ? "true" : undefined}
                    disabled={completeActionDisabled}
                    type="button"
                    onClick={completeCurrentSession}
                  >
                    <CheckCircle2 size={15} strokeWidth={2.1} />
                  </button>
                </GoodTooltip>
                if (canReconnect) {
                  <GoodTooltip ariaLabel="Reconnect session" content="Reconnect session">
                    <button
                      aria-label="Reconnect session"
                      class={styles.headerActionButton}
                      disabled={activeHeaderAction !== null}
                      type="button"
                      onClick={reconnectCurrentSession}
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
                      onClick={cancelActiveTurn}
                    >
                      <XCircle size={15} strokeWidth={2.1} />
                    </button>
                  </GoodTooltip>
                }
              </div>
            </div>
          </header>
          const headerActionError = headerAction.error.value

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
              olderHistory={{
                canLoad: canLoadOlderHistory,
                error: olderHistory.error.value
                  ? formatSessionRequestError(olderHistory.error.value)
                  : null,
                loading: olderHistory.isPending.value,
                onLoad: loadOlderHistory,
              }}
              onSelectPermissionOption={async (input) => {
                await respondSessionPermission({
                  id: chat.session.id,
                  requestId: input.requestId,
                  outcome: {
                    outcome: "selected",
                    optionId: input.optionId,
                  },
                })
              }}
              scrollCacheKey={`detail:session:${chat.session.id}:transcript`}
            />
          }
          <footer class={styles.composerDock}>
            const submitError = page.submitError.value

            if (submitError) {
              <div class={styles.composerError} role="alert">
                <strong class={styles.composerErrorTitle}>"Prompt wasn't sent"</strong>
                <span
                  class={styles.composerErrorDescription}
                >"Your draft is still here. Try again when the session is available."</span>
              </div>
            }
            <Composer
              loadSuggestions={async (input) => {
                const response = await goddardSdk.session.composerSuggestions({
                  id: chat.session.id,
                  trigger: input.trigger,
                  query: input.query,
                })

                return response.suggestions
              }}
              onPromptChange={() => {
                if (submitError) {
                  page.clearSubmitError()
                }
              }}
              onSubmit={async (prompt) => {
                page.clearSubmitError()

                try {
                  await submitSessionPrompt({
                    id: chat.session.id,
                    acpId: chat.session.acpSessionId,
                    prompt,
                  })
                  chat.toggleCompletedHidden(false)
                } catch (error) {
                  console.error("Failed to submit session prompt.", error)
                  page.markSubmitFailed(error)
                  throw error
                }
              }}
            />
          </footer>
        </SessionChatQueryDataProvider>
      } pending {
        <SessionChatStateMessage
          description="Loading the session transcript and current session details."
          title="Loading session"
        />
      } catch (_error, retry) {
        <SessionChatStateMessage
          actionLabel="Try again"
          description="The daemon-backed session or history request failed."
          onAction={() => {
            queryClient.invalidate(goddardSdk.session.get, [{ id: props.sessionId }])
            queryClient.invalidate(goddardSdk.session.history, [{ id: props.sessionId }])
            retry()
          }}
          title="Couldn't load session"
        />
      }
    </main>
  </SessionChatPageModelProvider>
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

function formatSessionRequestError(error: unknown) {
  if (error instanceof Error && error.message.trim().length > 0) {
    return error.message
  }

  return "The daemon rejected the request."
}

function isSessionChatHeaderActionError(error: unknown) {
  return typeof error === "object" && error !== null && "description" in error && "title" in error
}
