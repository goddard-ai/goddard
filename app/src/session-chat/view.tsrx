import { deriveSessionLaunchModelConfig, type DaemonSession } from "@goddard-ai/sdk"
import { useSignal } from "@preact/signals"
import {
  AlertCircle,
  CheckCircle2,
  ChevronDown,
  ChevronUp,
  CircleDot,
  FileDiff,
  GitFork,
  Laptop,
  LoaderCircle,
  PauseCircle,
  XCircle,
} from "lucide-react"
import { useEffect, useMemo, useState } from "preact/hooks"
import { castProtected, SigmaTarget, useSigmaSync } from "preact-sigma"

import { Composer } from "./composer.tsrx"
import { SessionChat } from "./model.ts"
import type { SessionChatPageEvents } from "./page-events.ts"
import { SessionChatPageModelProvider, useSessionChatPageModel } from "./page-model.ts"
import { SessionChatQueryDataProvider } from "./query-data.tsrx"
import type { SessionChatStatus } from "./model.ts"
import { Transcript } from "./transcript.tsrx"
import styles from "./view.style.ts"
import { useInbox, useWorkbenchTabSet } from "~/app-state-context.tsrx"
import { AppCommand, useAppCommand } from "~/commands/app-command.ts"
import { AppCommandTooltip } from "~/commands/app-command-tooltip.tsrx"
import { GoodTooltip } from "~/lib/good-tooltip.tsrx"
import { queryClient, useQueries } from "~/lib/query.ts"
import { useNearestProjectPath, useReportTabProject } from "~/projects/use-tab-project.ts"
import { goddardSdk } from "~/sdk.ts"
import {
  findSelectConfigOption,
  findSessionModeConfigOption,
} from "~/session-input/config-options.ts"
import { SessionInputSelect, type SessionInputSelectItem } from "~/session-input/select-menu.tsrx"
import {
  SessionConfigOptionSelector,
  SessionModeSelector,
} from "~/session-input/session-mode-selector.tsrx"
import {
  cancelSessionTurn,
  completeSession,
  reconnectSession,
  respondSessionPermission,
  setSessionConfigOption,
  setSessionModel,
  submitSessionPrompt,
} from "~/sessions/mutations.ts"
import { useWorkbenchCachedValue } from "~/use-workbench-cached-value.ts"

export default component SessionChatView(props: {
  projectPath: string | null;
  sessionId: DaemonSession["id"];
  sessionTitle?: string;
}) {
  <SessionChatPageModelProvider>
    const page = useSessionChatPageModel()
    const [submittedPromptScrollKey, setSubmittedPromptScrollKey] = useState<number | null>(null)
    const pageEvents = useMemo(() => new SigmaTarget<SessionChatPageEvents>(), [])

    <main class={styles.root}>
      try {
        const [sessionHistory, { session }, sessionWorktree] = useQueries(
          (query) => [
            query(goddardSdk.session.history, {
              params: [{ id: props.sessionId }],
              refetchOnWindowReactivate: false,
            }),
            query(goddardSdk.session.get, {
              params: [{ id: props.sessionId }],
            }),
            query(goddardSdk.session.worktree, {
              params: [{ id: props.sessionId }],
            }),
          ],
        )

        const chat = useWorkbenchCachedValue("sessionChat", () => {
          const chat = new SessionChat({
            history: sessionHistory,
            session,
          })

          return {
            setup: () => chat.setup(),
            value: chat,
          }
        })
        useSigmaSync(chat, { history: sessionHistory, session }, (payload) => {
          chat.syncLoadedData(payload)
        })
        const protectedChat = castProtected(chat)
        const transcriptMessages = chat.transcriptMessages
        const userPromptCount = transcriptMessages.filter(
          (message) => message.kind === "message" && message.role === "user",
        ).length

        const resolvedProjectPath = useNearestProjectPath(chat.session.cwd)
        useReportTabProject(resolvedProjectPath)

        const inbox = useInbox()
        useEffect(() => {
          void inbox.markSessionVisited(session.id)
        }, [inbox, session.id])

        const canLoadOlderHistory = chat.hasMore && chat.nextCursor !== null
        const canNavigatePrompts = userPromptCount > 0
        const canReconnectSession =
          chat.connection.reconnectable && !chat.session.activeDaemonSession &&
            chat.session.connectionMode === "live"
        const canSubmitPrompt = chat.session.activeDaemonSession || canReconnectSession

        function skipToPrompt(direction: "next" | "previous") {
          pageEvents.emit("scrollToPrompt", { direction })
        }

        useAppCommand(AppCommand.sessionChat.skipToPreviousPrompt, () => {
          skipToPrompt("previous")
        }, { active: canNavigatePrompts })
        useAppCommand(AppCommand.sessionChat.skipToNextPrompt, () => {
          skipToPrompt("next")
        }, { active: canNavigatePrompts })

        async function loadOlderHistory() {
          if (!canLoadOlderHistory) {
            return
          }

          try {
            await page.olderHistory.run(() => chat.loadOlderHistory())
          } catch (error) {
            console.error("Failed to load older session history.", error)
          }
        }

        <SessionChatQueryDataProvider value={{ chat: protectedChat, session, sessionHistory }}>
          <header class={styles.header} data-session-chat-header="true">
            <div class={styles.headerIdentity}>
              <p class={styles.headerRepository}>
                if (sessionWorktree.worktree) {
                  <GitFork aria-hidden={true} size={16} strokeWidth={2.1} />
                } else {
                  <Laptop aria-hidden={true} size={16} strokeWidth={2.1} />
                }
                {chat.repositoryLabel}
              </p>
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
                const workbenchTabSet = useWorkbenchTabSet()
                const activeHeaderAction = page.headerAction.activeKey.value

                <AppCommandTooltip
                  command={AppCommand.sessionChat.skipToPreviousPrompt}
                  label="Previous prompt"
                >
                  <button
                    aria-label="Scroll to previous user prompt"
                    class={styles.headerActionButton}
                    disabled={!canNavigatePrompts}
                    type="button"
                    onClick={() => {
                      AppCommand.sessionChat.skipToPreviousPrompt()
                    }}
                  >
                    <ChevronUp size={15} strokeWidth={2.1} />
                  </button>
                </AppCommandTooltip>

                <AppCommandTooltip
                  command={AppCommand.sessionChat.skipToNextPrompt}
                  label="Next prompt"
                >
                  <button
                    aria-label="Scroll to next user prompt"
                    class={styles.headerActionButton}
                    disabled={!canNavigatePrompts}
                    type="button"
                    onClick={() => {
                      AppCommand.sessionChat.skipToNextPrompt()
                    }}
                  >
                    <ChevronDown size={15} strokeWidth={2.1} />
                  </button>
                </AppCommandTooltip>

                function openSessionChanges() {
                  queryClient.invalidate(goddardSdk.session.changes, [{ id: chat.session.id }])
                  workbenchTabSet.openOrFocusTab({
                    kind: "sessionChanges",
                    payload: {
                      sessionId: chat.session.id,
                      sessionTitle: chat.title,
                      repositoryLabel: chat.repositoryLabel,
                    },
                  })
                }

                <GoodTooltip ariaLabel="View changes" content="View changes">
                  <button
                    aria-label="View session changes"
                    class={styles.headerActionButton}
                    type="button"
                    onClick={openSessionChanges}
                  >
                    <FileDiff size={15} strokeWidth={2.1} />
                  </button>
                </GoodTooltip>

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

                const completeCurrentSession = async () => {
                  if (chat.session.activeDaemonSession || chat.session.completedHidden) {
                    return
                  }

                  await page.headerAction.run("complete", async () => {
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

                const activeTurnId = chat.summary.activeTurnId
                const cancelRequestedTurnId = page.cancelRequestedTurnId.value
                const cancelAlreadyRequested =
                  activeTurnId !== null && cancelRequestedTurnId === activeTurnId
                const canCancelTurn =
                  activeTurnId !== null && chat.session.activeDaemonSession &&
                    chat.summary.status === "running" &&
                    !cancelAlreadyRequested

                if (canCancelTurn) {
                  const cancelActiveTurn = async () => {
                    await page.headerAction.run("cancel", async () => {
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
          const headerActionError = page.headerAction.error.value

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
              messages={transcriptMessages}
              olderHistory={{
                canLoad: canLoadOlderHistory,
                error: page.olderHistory.error.value
                  ? formatSessionRequestError(page.olderHistory.error.value)
                  : null,
                loading: page.olderHistory.isPending.value,
                onLoad: loadOlderHistory,
              }}
              {pageEvents}
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
              showThinkingLabel={chat.summary.showThinkingLabel}
              stickToBottomKey={submittedPromptScrollKey ?? undefined}
            />
          }
          <footer class={styles.composerDock}>
            const submitError = page.submitPrompt.error.value
            const modePickerOpen = useSignal(false)
            const modelPickerOpen = useSignal(false)
            const thinkingPickerOpen = useSignal(false)
            const sessionModelConfig = deriveSessionLaunchModelConfig({
              models: chat.session.models,
              configOptions: chat.session.configOptions,
            })
            const modeOption = findSessionModeConfigOption(sessionModelConfig.configOptions)
            const thinkingOption = findSelectConfigOption(
              sessionModelConfig.configOptions,
              "thought_level",
            )
            const modelItems: SessionInputSelectItem[] =
              sessionModelConfig.models?.availableModels.map(
                (model) => ({
                  value: model.modelId,
                  label: model.name,
                  detail: model.description ?? model.modelId,
                  searchText: [model.name, model.description, model.modelId].filter(Boolean).join(
                    "\n",
                  ),
                }),
              ) ??
                []
            const currentModelId = sessionModelConfig.models?.currentModelId ?? null

            async function applyResolvedSessionModel(input: {
              modelId?: string | null;
              thinkingValue?: string | null;
            }) {
              const selectedThinkingValue = input.thinkingValue ?? thinkingOption?.currentValue
              const selectedConfigOptions =
                thinkingOption && typeof selectedThinkingValue === "string"
                  ? [
                      {
                        configId: thinkingOption.id,
                        value: selectedThinkingValue,
                      },
                    ]
                  : []
              const resolvedSelection = sessionModelConfig.resolveSelection({
                modelId: input.modelId ?? currentModelId,
                configOptions: selectedConfigOptions,
              })
              let latestSession: DaemonSession | null = null

              const configOptionsToApply =
                input.thinkingValue === undefined
                  ? []
                  : resolvedSelection.initialConfigOptions ?? []

              for (const configOption of configOptionsToApply) {
                const result = await setSessionConfigOption({
                  id: chat.session.id,
                  configId: configOption.configId,
                  value: String(configOption.value),
                })
                latestSession = result.session
              }

              if (
                resolvedSelection.initialModelId &&
                  resolvedSelection.initialModelId !== chat.session.models?.currentModelId
              ) {
                const result = await setSessionModel({
                  id: chat.session.id,
                  modelId: resolvedSelection.initialModelId,
                })
                latestSession = result.session
              }

              if (latestSession) {
                chat.syncSession(latestSession)
              }
            }

            if (submitError) {
              <div class={styles.composerError} role="alert">
                <strong class={styles.composerErrorTitle}>"Prompt wasn't sent"</strong>
                <span class={styles.composerErrorDescription}>
                  "Your draft is still here. Try again when the session is available."
                </span>
              </div>
            }
            <Composer
              contextUsage={chat.summary.contextUsage}
              disabled={!canSubmitPrompt}
              footerControls={<tsrx>
                if (modeOption || modelItems.length > 0 || thinkingOption) {
                  <div class={styles.composerControls}>
                    if (modeOption) {
                      <SessionModeSelector
                        disabled={!chat.session.activeDaemonSession}
                        {modeOption}
                        open={modePickerOpen}
                        onValueChange={async (value) => {
                          try {
                            const result = await setSessionConfigOption({
                              id: chat.session.id,
                              configId: modeOption.id,
                              value,
                            })
                            chat.syncSession(result.session)
                          } catch (error) {
                            console.error("Failed to update session mode.", error)
                          }
                        }}
                      />
                    }
                    if (modelItems.length > 0) {
                      <SessionInputSelect
                        disabled={!chat.session.activeDaemonSession}
                        filterable={true}
                        items={modelItems}
                        label="Model"
                        open={modelPickerOpen}
                        placeholder="Use the adapter default model"
                        value={currentModelId}
                        onValueChange={async (value) => {
                          try {
                            await applyResolvedSessionModel({ modelId: value })
                          } catch (error) {
                            console.error("Failed to update session model.", error)
                          }
                        }}
                      />
                    }
                    if (thinkingOption) {
                      <SessionConfigOptionSelector
                        disabled={!chat.session.activeDaemonSession}
                        option={thinkingOption}
                        open={thinkingPickerOpen}
                        onValueChange={async (value) => {
                          try {
                            await applyResolvedSessionModel({ thinkingValue: value })
                          } catch (error) {
                            console.error("Failed to update session thinking level.", error)
                          }
                        }}
                      />
                    }
                  </div>
                }
              </tsrx>}
              helperText={null}
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
                  page.submitPrompt.clearError()
                }
              }}
              onSubmit={async (prompt) => {
                setSubmittedPromptScrollKey((currentKey) => (currentKey ?? 0) + 1)
                await page.submitPrompt.run(async () => {
                  if (canReconnectSession) {
                    const { session } = await reconnectSession(chat.session.id)
                    chat.syncSession(session)
                  }

                  await submitSessionPrompt({
                    id: chat.session.id,
                    acpId: chat.session.acpSessionId,
                    prompt,
                  })
                  chat.toggleCompletedHidden(false)
                }).catch((error) => {
                  console.error("Failed to submit session prompt.", error)
                  throw error
                })
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
