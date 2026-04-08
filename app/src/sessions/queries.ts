import type { DaemonSession } from "@goddard-ai/sdk"
import { goddardSdk } from "~/sdk.ts"

export const SESSION_LIST_LIMIT = 50
const sessionsRootKey = ["sessions"] as const

export const sessionQueryKeys = {
  all: sessionsRootKey,
  lists: () => [...sessionsRootKey, "list"] as const,
  list: (limit: number) => [...sessionsRootKey, "list", limit] as const,
  details: () => [...sessionsRootKey, "detail"] as const,
  detail: (sessionId: DaemonSession["id"]) => [...sessionsRootKey, "detail", sessionId] as const,
  histories: () => [...sessionsRootKey, "history"] as const,
  history: (sessionId: DaemonSession["id"]) => [...sessionsRootKey, "history", sessionId] as const,
}

export function getSessionsListQueryOptions(limit = SESSION_LIST_LIMIT) {
  return {
    queryKey: sessionQueryKeys.list(limit),
    queryFn: async () => {
      const response = await goddardSdk.session.list({ limit })
      return response.sessions
    },
  }
}

export function getSessionQueryOptions(sessionId: DaemonSession["id"]) {
  return {
    queryKey: sessionQueryKeys.detail(sessionId),
    queryFn: async () => {
      const response = await goddardSdk.session.get({ id: sessionId })
      return response.session
    },
  }
}

export function getSessionHistoryQueryOptions(sessionId: DaemonSession["id"]) {
  return {
    queryKey: sessionQueryKeys.history(sessionId),
    queryFn: async () => {
      return await goddardSdk.session.history({ id: sessionId })
    },
  }
}
