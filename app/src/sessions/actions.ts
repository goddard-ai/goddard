import type { CreateSessionRequest, DaemonSession, SessionPromptRequest } from "@goddard-ai/sdk"

import { createActionsProvider } from "~/lib/actions-provider.tsx"
import { queryClient } from "~/lib/query.ts"
import { goddardSdk } from "~/sdk.ts"
import { SESSION_LIST_LIMIT } from "./queries.ts"

/**
 * Creates one session and refreshes the visible session list afterwards.
 */
export async function createSession(input: CreateSessionRequest) {
  const result = await goddardSdk.session.create(input)
  queryClient.invalidate(goddardSdk.session.list, [{ limit: SESSION_LIST_LIMIT }])
  return result
}

/**
 * Submits one prompt into an existing session and refreshes the affected session views.
 */
export async function submitSessionPrompt(props: SessionPromptRequest) {
  await goddardSdk.session.prompt(props)
  queryClient.invalidate(goddardSdk.session.list, [{ limit: SESSION_LIST_LIMIT }])
  queryClient.invalidate(goddardSdk.session.get, [{ id: props.id }])
  queryClient.invalidate(goddardSdk.session.history, [{ id: props.id }])
}

export const SessionsPageActions = createActionsProvider<{
  openSession: (sessionId: DaemonSession["id"]) => void
  openSessionChanges: (sessionId: DaemonSession["id"]) => void
}>("SessionsPageActions")
