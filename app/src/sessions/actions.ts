import type { CreateSessionRequest, DaemonSession } from "@goddard-ai/sdk"
import { getSession, getSessionHistory, listSessions, SESSION_LIST_LIMIT } from "./queries.ts"
import { queryClient } from "~/lib/query.ts"
import { goddardSdk } from "~/sdk.ts"

/**
 * Creates one session and refreshes the visible session list afterwards.
 */
export async function createSession(input: CreateSessionRequest) {
  const result = await goddardSdk.session.create(input)
  queryClient.invalidate(listSessions, [SESSION_LIST_LIMIT])
  return result
}

/**
 * Submits one prompt into an existing session and refreshes the affected session views.
 */
export async function submitSessionPrompt(props: {
  acpId: DaemonSession["acpSessionId"]
  id: DaemonSession["id"]
  prompt: string
}) {
  await goddardSdk.session.prompt(props)
  queryClient.invalidate(listSessions, [SESSION_LIST_LIMIT])
  queryClient.invalidate(getSession, [props.id])
  queryClient.invalidate(getSessionHistory, [props.id])
}
