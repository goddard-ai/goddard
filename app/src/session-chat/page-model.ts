import { signal } from "@preact/signals"

import { createPageModelContext } from "~/lib/page-model-context.tsrx"
import { createTask } from "~/lib/task.ts"

export type SessionChatHeaderAction = "cancel" | "complete" | "reconnect"

export type SessionChatHeaderActionError = {
  description: string
  title: string
}

export const {
  PageModelProvider: SessionChatPageModelProvider,
  usePageModel: useSessionChatPageModel,
} = createPageModelContext(function () {
  const retryVersion = signal(0)
  const submitError = signal<unknown>(null)
  const activeHeaderAction = signal<SessionChatHeaderAction | null>(null)
  const olderHistory = createTask()
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
    headerActionError,
    markCancelRequested(turnId: string) {
      cancelRequestedTurnId.value = turnId
    },
    markSubmitFailed(error: unknown) {
      submitError.value = error
    },
    olderHistory,
    retryLoad() {
      retryVersion.value += 1
    },
    retryVersion,
    startHeaderAction(action: SessionChatHeaderAction) {
      activeHeaderAction.value = action
      headerActionError.value = null
    },
    submitError,
  }
})
