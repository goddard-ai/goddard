import type { CreateSessionRequest, DaemonSession } from "@goddard-ai/sdk"
import { SESSION_LIST_LIMIT } from "./queries.ts"
import { queryClient } from "~/lib/query.ts"
import { goddardSdk } from "~/sdk.ts"

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
export async function submitSessionPrompt(props: {
  acpId: DaemonSession["acpSessionId"]
  id: DaemonSession["id"]
  prompt: string
}) {
  await goddardSdk.session.prompt(props)
  queryClient.invalidate(goddardSdk.session.list, [{ limit: SESSION_LIST_LIMIT }])
  queryClient.invalidate(goddardSdk.session.get, [{ id: props.id }])
  queryClient.invalidate(goddardSdk.session.history, [{ id: props.id }])
}
