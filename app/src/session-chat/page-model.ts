import { signal } from "@preact/signals"

import { createPageModelContext } from "~/lib/page-model-context.tsrx"
import { createKeyedTask, createTask } from "~/lib/task.ts"

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
  const headerAction = createKeyedTask<SessionChatHeaderAction, SessionChatHeaderActionError>()
  const olderHistory = createTask()
  const cancelRequestedTurnId = signal<string | null>(null)

  return {
    cancelRequestedTurnId,
    clearSubmitError() {
      submitError.value = null
    },
    headerAction,
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
    submitError,
  }
})
