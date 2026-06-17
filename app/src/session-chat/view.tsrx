import { deriveSessionLaunchModelConfig, type DaemonSession } from "@goddard-ai/sdk"
import { cx } from "@goddard-ai/styled-system/css"
import { useSignal } from "@preact/signals"
import {
  CheckCircle2,
  ChevronDown,
  ChevronUp,
  FileDiff,
  GitFork,
  Laptop,
  XCircle,
} from "lucide-react"
import { useEffect, useMemo, useState } from "preact/hooks"
import { castProtected, SigmaTarget, useSigmaSync } from "preact-sigma"
import { isObject } from "radashi"

import { Composer } from "./composer.tsrx"
import { loadSessionChatComposerSuggestions } from "./composer-suggestion-loader.ts"
import { SessionChat } from "./model.ts"
import type { SessionChatPageEvents } from "./page-events.ts"
import { SessionChatPageModelProvider, useSessionChatPageModel } from "./page-model.ts"
import { SessionChatQueryDataProvider } from "./query-data.tsrx"
import { Transcript } from "./transcript.tsrx"
import styles from "./view.style.ts"
import { useShortcutRegistry, useWorkbenchTabSet } from "~/app-state-context.tsrx"
import { markInboxSessionVisited } from "~/inbox/session-visit.ts"
import { AppCommand, useAppCommand } from "~/commands/app-command.ts"
import { AppCommandTooltip } from "~/commands/app-command-tooltip.tsrx"
import { getPreferredShortcutLabel, getShortcutLabel } from "~/commands/shortcut-label.ts"
import { text } from "~/language/text.ts"
import { GoodTooltip } from "~/lib/good-tooltip.tsrx"
import { useErrorBoundaryReset } from "~/lib/error-boundary-reset.ts"
import { useQueries, useQuery } from "~/lib/query.ts"
import { goddardSdk } from "~/sdk.ts"
import {
  findSelectConfigOption,
  findSessionModeConfigOption,
  stepConfigOptionValue,
} from "~/session-input/config-options.ts"
import { SessionInputSelect, type SessionInputSelectItem } from "~/session-input/select-menu.tsrx"
import { AcpAgentIcon } from "~/sessions/acp-agent-icon.tsrx"
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
  submitSessionPrompt,
} from "~/sessions/mutations.ts"
import { invalidateSessionChanges, invalidateSessionDetail } from "~/sessions/cache.ts"
import { useWorkbenchCachedValue } from "~/use-workbench-cached-value.ts"

export default function SessionChatView(props: {
  relatedFilesystemPath: string | null;
  sessionId: DaemonSession["id"];
  sessionTitle?: string;
}) @{
  <SessionChatPageModelProvider>
    <SessionChatViewContent {...props} />
  </SessionChatPageModelProvider>
}

function SessionChatViewContent(props: {
  relatedFilesystemPath: string | null;
  sessionId: DaemonSession["id"];
  sessionTitle?: string;
}) @{
  const page = useSessionChatPageModel()
  const [submittedPromptScrollKey, setSubmittedPromptScrollKey] = useState<number | null>(null)
  const pageEvents = useMemo(() => new SigmaTarget<SessionChatPageEvents>(), [])

  <main class={styles.root}>
    @try {
      const [sessionHistory, { session }, sessionWorktree] = useQueries(
        (query) => [
          query(goddardSdk.session.history, {
            params: [{ id: props.sessionId }],
            refetchOnWindowReactivate: false,
          }),
          query(goddardSdk.session.get, {
            params: [{ id: props.sessionId }],
          }),
          query(goddardSdk.session.worktree.get, {
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

      useEffect(() => {
        markInboxSessionVisited(session.id)
      }, [session.id])

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

      const headerActionError = page.headerAction.error.value

      <SessionChatQueryDataProvider value={{ chat: protectedChat, session, sessionHistory }}>
        <header class={cx(styles.header, "session-chat-header")}>
          <div class={styles.headerIdentity}>
            <p class={styles.headerRepository}>
              @if (sessionWorktree.worktree) {
                <GitFork aria-hidden={true} size={16} strokeWidth={2.1} />
              } @else {
                <Laptop aria-hidden={true} size={16} strokeWidth={2.1} />
              }

              {chat.repositoryLabel}
            </p>
          </div>
          <div class={styles.headerControls}>
            <div class={styles.headerActions}>
              <SessionChatHeaderActions {canNavigatePrompts} {chat} {page} />
            </div>
          </div>
        </header>

        {headerActionError
          ? <div class={styles.headerActionError} role="alert">
              <strong class={styles.headerActionErrorTitle}>{headerActionError.title}</strong>
              <span
                class={styles.headerActionErrorDescription}
              >{headerActionError.description}</span>
            </div>
          : null}

        @if (chat.hasEmptyTranscript) {
          <div class={styles.transcript}>
            <SessionChatStateMessage
              description="Send a prompt to start this session."
              title="No messages yet"
            />
          </div>
        } @else {
          <Transcript
            class={styles.transcript}
            initialScrollPosition="bottom"
            messages={transcriptMessages}
            olderHistory={{
              canLoad: canLoadOlderHistory,
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
            showThinkingLabel={chat.summary.showThinkingLabel}
            stickToBottomKey={submittedPromptScrollKey ?? undefined}
          />
        }

        <SessionChatComposerDock
          {canReconnectSession}
          {canSubmitPrompt}
          {chat}
          {page}
          onSubmittedPrompt={() => {
            setSubmittedPromptScrollKey((currentKey) => (currentKey ?? 0) + 1)
          }}
        />
      </SessionChatQueryDataProvider>
    } @pending {
      <SessionChatStateMessage
        description="Loading the session transcript and current session details."
        title="Loading session"
      />
    } @catch (_error, retry) {
      const resetSessionChatBoundary = () => {
        invalidateSessionDetail(props.sessionId)
        retry()
      }
      useErrorBoundaryReset(resetSessionChatBoundary)

      <SessionChatStateMessage
        actionLabel="Try again"
        description="The daemon-backed session or history request failed."
        onAction={resetSessionChatBoundary}
        title="Couldn't load session"
      />
    }
  </main>
}

function SessionChatHeaderActions(props: {
  canNavigatePrompts: boolean;
  chat: SessionChat;
  page: ReturnType<typeof useSessionChatPageModel>;
}) @{
  const workbenchTabSet = useWorkbenchTabSet()
  const activeHeaderAction = props.page.headerAction.activeKey.value

  function openSessionChanges() {
    invalidateSessionChanges(props.chat.session.id)
    workbenchTabSet.openOrFocusTab({
      kind: "sessionChanges",
      props: {
        sessionId: props.chat.session.id,
        sessionTitle: props.chat.title,
        repositoryLabel: props.chat.repositoryLabel,
      },
    })
  }

  useAppCommand(AppCommand.sessionChat.viewChanges, openSessionChanges)

  const completeActionDisabled =
    activeHeaderAction !== null || props.chat.session.activeDaemonSession ||
      props.chat.session.completedHidden
  const completeActionLabel =
    props.chat.session.completedHidden
      ? text.sessionCompleted
      : props.chat.session.activeDaemonSession
        ? text.completeSessionUnavailableWhileAgentIsActive
        : activeHeaderAction === "complete"
          ? text.completingSession
          : text.completeSession

  const completeCurrentSession = async () => {
    if (props.chat.session.activeDaemonSession || props.chat.session.completedHidden) {
      return
    }

    await props.page.headerAction.run("complete", async () => {
      try {
        await completeSession(props.chat.session.id)
        props.chat.toggleCompletedHidden(true)
      } catch (error) {
        console.error("Failed to complete session.", error)
        throw {
          title: text.completeFailed,
          description: formatSessionRequestError(error),
        }
      }
    }).catch(() => {})
  }

  useAppCommand(AppCommand.sessionChat.completeSession, completeCurrentSession, {
    active: !completeActionDisabled,
  })

  const activeTurnId = props.chat.summary.activeTurnId
  const cancelRequestedTurnId = props.page.cancelRequestedTurnId.value
  const cancelAlreadyRequested = activeTurnId !== null && cancelRequestedTurnId === activeTurnId
  const canCancelTurn =
    activeTurnId !== null && props.chat.session.activeDaemonSession &&
      props.chat.summary.status === "running" &&
      !cancelAlreadyRequested

  const cancelActiveTurn = async () => {
    if (activeTurnId === null) {
      return
    }

    await props.page.headerAction.run("cancel", async () => {
      try {
        const result = await cancelSessionTurn(props.chat.session.id)

        if (!result.activeTurnCancelled && result.abortedQueue.length === 0) {
          throw {
            title: text.stopSkipped,
            description: text.theDaemonDidNotFindAnActiveTurnToStop,
          }
        }

        props.page.markCancelRequested(activeTurnId)
      } catch (error) {
        if (isSessionChatHeaderActionError(error)) {
          throw error
        }

        console.error("Failed to stop session turn.", error)
        throw {
          title: text.stopFailed,
          description: formatSessionRequestError(error),
        }
      }
    }).catch(() => {})
  }

  <>
    <AppCommandTooltip
      command={AppCommand.sessionChat.skipToPreviousPrompt}
      label={text.previousPrompt}
    >
      <button
        aria-label={text.scrollToPreviousUserPrompt}
        class={styles.headerActionButton}
        disabled={!props.canNavigatePrompts}
        type="button"
        onClick={() => {
          AppCommand.sessionChat.skipToPreviousPrompt()
        }}
      >
        <ChevronUp size={15} strokeWidth={2.1} />
      </button>
    </AppCommandTooltip>

    <AppCommandTooltip command={AppCommand.sessionChat.skipToNextPrompt} label={text.nextPrompt}>
      <button
        aria-label={text.scrollToNextUserPrompt}
        class={styles.headerActionButton}
        disabled={!props.canNavigatePrompts}
        type="button"
        onClick={() => {
          AppCommand.sessionChat.skipToNextPrompt()
        }}
      >
        <ChevronDown size={15} strokeWidth={2.1} />
      </button>
    </AppCommandTooltip>

    <AppCommandTooltip command={AppCommand.sessionChat.viewChanges}>
      <button
        aria-label={text.viewSessionChanges}
        class={styles.headerActionButton}
        type="button"
        onClick={() => {
          AppCommand.sessionChat.viewChanges()
        }}
      >
        <FileDiff size={15} strokeWidth={2.1} />
      </button>
    </AppCommandTooltip>

    <AppCommandTooltip command={AppCommand.sessionChat.completeSession} label={completeActionLabel}>
      <button
        aria-label={completeActionLabel}
        class={styles.headerActionButton}
        data-active={props.chat.session.completedHidden ? "true" : undefined}
        disabled={completeActionDisabled}
        type="button"
        onClick={() => {
          AppCommand.sessionChat.completeSession()
        }}
      >
        <CheckCircle2 size={15} strokeWidth={2.1} />
      </button>
    </AppCommandTooltip>

    {canCancelTurn
      ? <GoodTooltip ariaLabel={text.stopActiveTurn} content={text.stopActiveTurn}>
          <button
            aria-label={text.stopActiveTurn}
            class={styles.headerActionButton}
            data-tone="danger"
            disabled={activeHeaderAction !== null}
            type="button"
            onClick={cancelActiveTurn}
          >
            <XCircle size={15} strokeWidth={2.1} />
          </button>
        </GoodTooltip>
      : null}
  </>
}

function SessionChatComposerDock(props: {
  canReconnectSession: boolean;
  canSubmitPrompt: boolean;
  chat: SessionChat;
  onSubmittedPrompt: () => void;
  page: ReturnType<typeof useSessionChatPageModel>;
}) @{
  const submitError = props.page.submitPrompt.error.value
  const modePickerOpen = useSignal(false)
  const modelPickerOpen = useSignal(false)
  const thinkingPickerOpen = useSignal(false)
  const draftModeValue = useSignal<string | null>(null)
  const draftModelId = useSignal<string | null>(null)
  const draftThinkingValue = useSignal<string | null>(null)
  const shortcutRegistry = useShortcutRegistry()
  const sessionModelConfig = deriveSessionLaunchModelConfig({
    configOptions: props.chat.session.configOptions,
  })
  const adapterCatalog = useQuery(goddardSdk.adapter.list, {
    params: [
      { cwd: props.chat.session.cwd, includeUninstalled: true },
    ],
  })
  const agentAdapter =
    typeof props.chat.session.agent === "string"
      ? adapterCatalog.adapters.find((adapter) => adapter.id === props.chat.session.agent) ?? null
      : null
  const agentIcon =
    typeof props.chat.session.agent === "string"
      ? agentAdapter?.icon ?? null
      : props.chat.session.agent?.icon ?? null
  const agentVersion =
    typeof props.chat.session.agent === "string"
      ? agentAdapter?.version ?? null
      : props.chat.session.agent?.version ?? null
  const modeOption = findSessionModeConfigOption(sessionModelConfig.configOptions)
  const thinkingOption = findSelectConfigOption(sessionModelConfig.configOptions, "thought_level")
  const modelItems: SessionInputSelectItem[] =
    sessionModelConfig.models?.availableModels.map(
      (model) => ({
        value: model.modelId,
        label: model.name,
        detail: model.description ?? model.modelId,
        searchText: [model.name, model.description, model.modelId].filter(Boolean).join("\n"),
      }),
    ) ??
      []
  const currentModelId = sessionModelConfig.models?.currentModelId ?? null
  const approvalPresetShortcut = getShortcutLabel(
    shortcutRegistry.resolvedBindings[AppCommand.sessionInput.openApprovalPresetSelector.id]?.[0],
  )
  const modelShortcut = getPreferredShortcutLabel(
    shortcutRegistry.resolvedBindings[AppCommand.sessionInput.openModelSelector.id],
    "Mod+/",
  )
  const thinkingShortcut = getShortcutLabel(
    shortcutRegistry.resolvedBindings[AppCommand.sessionInput.openThinkingLevelSelector.id]?.[0],
  )
  const hasFooterControls =
    props.chat.session.agentName || modeOption || modelItems.length > 0 || thinkingOption
  const footerControls =
    hasFooterControls
      ? <div class={styles.composerControls}>
          {props.chat.session.agentName
            ? <GoodTooltip
                ariaLabel={`Agent harness: ${props.chat.session.agentName}`}
                content={<span class={styles.composerAgentTooltip}>
                  <span>{props.chat.session.agentName}</span>
                  {agentVersion
                    ? <span class={styles.composerAgentTooltipVersion}>{agentVersion}</span>
                    : null}
                </span>}
              >
                <div
                  aria-label={`Agent harness: ${props.chat.session.agentName}`}
                  class={styles.composerAgentControl}
                >
                  <AcpAgentIcon class={styles.composerAgentIcon} icon={agentIcon} />
                </div>
              </GoodTooltip>
            : null}
          {modelItems.length > 0
            ? <SessionInputSelect
                compact={true}
                filterable={true}
                items={modelItems}
                label={text.model}
                open={modelPickerOpen}
                placeholder={text.useTheAgentHarnessDefaultModel}
                shortcut={modelShortcut}
                value={draftModelId.value ?? currentModelId}
                onValueChange={(value) => {
                  draftModelId.value = value
                }}
              />
            : null}
          {thinkingOption
            ? <SessionConfigOptionSelector
                compact={true}
                option={{
                  ...thinkingOption,
                  currentValue: draftThinkingValue.value ?? thinkingOption.currentValue,
                }}
                open={thinkingPickerOpen}
                shortcut={thinkingShortcut}
                onValueChange={(value) => {
                  draftThinkingValue.value = value
                }}
              />
            : null}
          {modeOption
            ? <div class={styles.composerApprovalControl}>
                <SessionModeSelector
                  compact={true}
                  modeOption={{
                    ...modeOption,
                    currentValue: draftModeValue.value ?? modeOption.currentValue,
                  }}
                  open={modePickerOpen}
                  shortcut={approvalPresetShortcut}
                  onValueChange={(value) => {
                    draftModeValue.value = value
                  }}
                />
              </div>
            : null}
        </div>
      : null

  useAppCommand(AppCommand.sessionInput.openApprovalPresetSelector, () => {
    modePickerOpen.value = true
    modelPickerOpen.value = false
    thinkingPickerOpen.value = false
  }, { active: modeOption !== null })
  useAppCommand(AppCommand.sessionInput.openModelSelector, () => {
    modePickerOpen.value = false
    modelPickerOpen.value = true
    thinkingPickerOpen.value = false
  }, { active: modelItems.length > 0 })
  useAppCommand(AppCommand.sessionInput.openThinkingLevelSelector, () => {
    modePickerOpen.value = false
    thinkingPickerOpen.value = true
    modelPickerOpen.value = false
  }, { active: thinkingOption !== null })
  useAppCommand(AppCommand.sessionInput.decreaseThinkingLevel, () => {
    if (!thinkingOption) {
      return
    }

    draftThinkingValue.value = stepConfigOptionValue(
      thinkingOption,
      draftThinkingValue.value ?? thinkingOption.currentValue,
      -1,
    )
  }, { active: thinkingOption !== null })
  useAppCommand(AppCommand.sessionInput.increaseThinkingLevel, () => {
    if (!thinkingOption) {
      return
    }

    draftThinkingValue.value = stepConfigOptionValue(
      thinkingOption,
      draftThinkingValue.value ?? thinkingOption.currentValue,
      1,
    )
  }, { active: thinkingOption !== null })

  async function applyDraftSessionControls() {
    const selectedModeValue = draftModeValue.value ?? modeOption?.currentValue ?? null
    const selectedThinkingValue = draftThinkingValue.value ?? thinkingOption?.currentValue ?? null
    const shouldApplyMode =
      modeOption && draftModeValue.value !== null && selectedModeValue !== modeOption.currentValue
    const shouldApplyThinking =
      thinkingOption && draftThinkingValue.value !== null &&
        selectedThinkingValue !== thinkingOption.currentValue
    const shouldApplyModel = draftModelId.value !== null || shouldApplyThinking
    const selectedConfigOptions =
      thinkingOption && shouldApplyThinking && typeof selectedThinkingValue === "string"
        ? [
            {
              configId: thinkingOption.id,
              value: selectedThinkingValue,
            },
          ]
        : []
    const resolvedSelection = sessionModelConfig.resolveSelection({
      modelId: draftModelId.value,
      configOptions: selectedConfigOptions,
    })
    let latestSession: DaemonSession | null = null

    if (shouldApplyMode && typeof selectedModeValue === "string") {
      const result = await setSessionConfigOption({
        id: props.chat.session.id,
        configId: modeOption.id,
        value: selectedModeValue,
      })
      latestSession = result.session
    }

    if (shouldApplyModel) {
      for (const configOption of resolvedSelection.initialConfigOptions ?? []) {
        const result = await setSessionConfigOption({
          id: props.chat.session.id,
          configId: configOption.configId,
          value: String(configOption.value),
        })
        latestSession = result.session
      }
    }

    if (latestSession) {
      props.chat.syncSession(latestSession)
    }

    draftModeValue.value = null
    draftModelId.value = null
    draftThinkingValue.value = null
  }

  <footer class={styles.composerDock}>
    {submitError
      ? <div class={styles.composerError} role="alert">
          <strong class={styles.composerErrorTitle}>{text.promptWasntSent}</strong>
          <span
            class={styles.composerErrorDescription}
          >{text.yourDraftIsStillHereTryAgainWhenTheSessionIsAvailable}</span>
        </div>
      : null}

    <Composer
      contextUsage={props.chat.summary.contextUsage}
      disabled={!props.canSubmitPrompt}
      footerControls={footerControls}
      helperText={null}
      loadSuggestions={async (input) => {
        return loadSessionChatComposerSuggestions({
          cwd: props.chat.session.cwd,
          sdk: goddardSdk,
          sessionId: props.chat.session.id,
          trigger: input.trigger,
          query: input.query,
        })
      }}
      onPromptChange={() => {
        if (submitError) {
          props.page.submitPrompt.clearError()
        }
      }}
      onSubmit={async (prompt) => {
        props.onSubmittedPrompt()
        await props.page.submitPrompt.run(async () => {
          if (props.canReconnectSession) {
            const { session } = await reconnectSession(props.chat.session.id)
            props.chat.syncSession(session)
          }

          await applyDraftSessionControls()
          await submitSessionPrompt({
            id: props.chat.session.id,
            acpId: props.chat.session.acpSessionId,
            prompt,
          })
          props.chat.toggleCompletedHidden(false)
        }).catch((error) => {
          console.error("Failed to submit session prompt.", error)
          throw error
        })
      }}
    />
  </footer>
}

function SessionChatStateMessage(props: {
  actionLabel?: string;
  description: string;
  onAction?: () => void;
  title: string;
}) @{
  <div class={styles.stateMessage}>
    <h2 class={styles.stateTitle}>{props.title}</h2>
    <p class={styles.stateDescription}>{props.description}</p>

    {props.actionLabel && props.onAction
      ? <button
          class={styles.stateAction}
          type="button"
          onClick={props.onAction}
        >{props.actionLabel}</button>
      : null}
  </div>
}

function formatSessionRequestError(error: unknown) {
  if (error instanceof Error && error.message.trim().length > 0) {
    return error.message
  }

  return "The daemon rejected the request."
}

function isSessionChatHeaderActionError(error: unknown) {
  return isObject(error) && "description" in error && "title" in error
}
