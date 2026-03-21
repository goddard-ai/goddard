import { $type, route } from "rouzer"
import { z } from "zod"
import { RepoEventHistoryQuery, type RepoEventHistoryResponse } from "../repo-events.js"

const RepoStreamHeaders = z.object({
  authorization: z.string().optional(),
})

const RepoStreamQuery = z.object({
  token: z.string().optional(),
})

/** Opens the authenticated user-scoped feedback stream. */
export const repoStreamRoute = route("stream", {
  GET: {
    headers: RepoStreamHeaders,
    query: RepoStreamQuery,
  },
})

/** Lists persisted managed pull-request events after one cursor for the current user. */
export const repoStreamHistoryRoute = route("stream/history", {
  GET: {
    headers: z.object({
      authorization: z.string(),
    }),
    query: RepoEventHistoryQuery,
    response: $type<RepoEventHistoryResponse>(),
  },
})
