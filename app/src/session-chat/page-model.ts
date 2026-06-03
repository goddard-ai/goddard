import { signal } from "@preact/signals"

import { createPageModelContext } from "~/lib/page-model-context.tsrx"
import { createKeyedTask, createTask } from "~/lib/task.ts"

export type SessionChatHeaderAction = "archive" | "cancel" | "complete" | "reconnect" | "unarchive"

export type SessionChatHeaderActionError = {
  description: string
  title: string
}

export const {
  PageModelProvider: SessionChatPageModelProvider,
  usePageModel: useSessionChatPageModel,
} = createPageModelContext(function () {
  const headerAction = createKeyedTask<SessionChatHeaderAction, SessionChatHeaderActionError>()
  const olderHistory = createTask()
  const cancelRequestedTurnId = signal<string | null>(null)
  const submitPrompt = createTask()

  return {
    cancelRequestedTurnId,
    headerAction,
    markCancelRequested(turnId: string) {
      cancelRequestedTurnId.value = turnId
    },
    olderHistory,
    submitPrompt,
  }
})
