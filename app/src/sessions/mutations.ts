import type { CreateSessionRequest, DaemonSession, SessionPromptRequest } from "@goddard-ai/sdk"

import { createMutationsProvider } from "~/lib/mutations-provider.tsx"
import { queryClient } from "~/lib/query.ts"
import { goddardSdk } from "~/sdk.ts"
import { SESSION_LIST_LIMIT } from "./queries.ts"

function refreshSessionViews(sessionId: DaemonSession["id"]) {
  queryClient.invalidate(goddardSdk.session.list, [{ limit: SESSION_LIST_LIMIT }])
  queryClient.invalidate(goddardSdk.session.get, [{ id: sessionId }])
  queryClient.invalidate(goddardSdk.session.history, [{ id: sessionId }])
}

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
  refreshSessionViews(props.id)
}

/**
 * Reconnects one loadable session and refreshes the affected session views.
 */
export async function reconnectSession(sessionId: DaemonSession["id"]) {
  const result = await goddardSdk.session.connect({ id: sessionId })
  refreshSessionViews(sessionId)
  return result
}

/**
 * Cancels the active turn for one session and refreshes the affected session views.
 */
export async function cancelSessionTurn(sessionId: DaemonSession["id"]) {
  const result = await goddardSdk.session.cancel({ id: sessionId })
  refreshSessionViews(sessionId)
  return result
}

export const SessionsPageMutations = createMutationsProvider<{
  openSession: (sessionId: DaemonSession["id"]) => void
  openSessionChanges: (sessionId: DaemonSession["id"]) => void
}>("SessionsPageMutations")
