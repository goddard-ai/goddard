import type { DaemonSession } from "@goddard-ai/sdk"
import { goddardSdk } from "~/sdk.ts"

export const SESSION_LIST_LIMIT = 50

export async function listSessions(limit = SESSION_LIST_LIMIT) {
  const response = await goddardSdk.session.list({ limit })
  return response.sessions
}

export async function getSession(sessionId: DaemonSession["id"]) {
  const response = await goddardSdk.session.get({ id: sessionId })
  return response.session
}

export async function getSessionHistory(sessionId: DaemonSession["id"]) {
  return await goddardSdk.session.history({ id: sessionId })
}

export async function getOptionalSessionHistory(sessionId: DaemonSession["id"] | null) {
  if (!sessionId) {
    return null
  }

  return await getSessionHistory(sessionId)
}
