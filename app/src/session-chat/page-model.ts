import { signal, type Signal } from "@preact/signals"

import { createPageModelContext } from "~/lib/page-model-context.tsrx"

export type SessionChatHeaderAction = "cancel" | "complete" | "reconnect"

export type SessionChatOlderHistoryState =
  | { status: "idle" }
  | { status: "loading" }
  | { message: string; status: "error" }

export type SessionChatHeaderActionError = {
  description: string
  title: string
}

type SessionChatPageModel = {
  activeHeaderAction: Signal<SessionChatHeaderAction | null>
  cancelRequestedTurnId: Signal<string | null>
  clearHeaderAction: () => void
  clearSubmitError: () => void
  failHeaderAction: (error: SessionChatHeaderActionError) => void
  failOlderHistoryLoad: (message: string) => void
  finishOlderHistoryLoad: () => void
  headerActionError: Signal<SessionChatHeaderActionError | null>
  markCancelRequested: (turnId: string) => void
  markSubmitFailed: (error: unknown) => void
  olderHistoryState: Signal<SessionChatOlderHistoryState>
  retryLoad: () => void
  retryVersion: Signal<number>
  startHeaderAction: (action: SessionChatHeaderAction) => void
  startOlderHistoryLoad: () => void
  submitError: Signal<unknown>
}

export const {
  PageModelProvider: SessionChatPageModelProvider,
  usePageModel: useSessionChatPageModel,
} = createPageModelContext<SessionChatPageModel>(function () {
  const retryVersion = signal(0)
  const submitError = signal<unknown>(null)
  const activeHeaderAction = signal<SessionChatHeaderAction | null>(null)
  const olderHistoryState = signal<SessionChatOlderHistoryState>({ status: "idle" })
  const headerActionError = signal<SessionChatHeaderActionError | null>(null)
  const cancelRequestedTurnId = signal<string | null>(null)

  return {
    activeHeaderAction,
    cancelRequestedTurnId,
    clearHeaderAction() {
      activeHeaderAction.value = null
    },
    clearSubmitError() {
      submitError.value = null
    },
    failHeaderAction(error: SessionChatHeaderActionError) {
      headerActionError.value = error
    },
    failOlderHistoryLoad(message: string) {
      olderHistoryState.value = {
        status: "error",
        message,
      }
    },
    finishOlderHistoryLoad() {
      olderHistoryState.value = { status: "idle" }
    },
    headerActionError,
    markCancelRequested(turnId: string) {
      cancelRequestedTurnId.value = turnId
    },
    markSubmitFailed(error: unknown) {
      submitError.value = error
    },
    olderHistoryState,
    retryLoad() {
      retryVersion.value += 1
    },
    retryVersion,
    startHeaderAction(action: SessionChatHeaderAction) {
      activeHeaderAction.value = action
      headerActionError.value = null
    },
    startOlderHistoryLoad() {
      olderHistoryState.value = { status: "loading" }
    },
    submitError,
  }
})
