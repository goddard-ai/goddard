import { $type, route } from "rouzer"
import { BearerHeaders } from "../../common/auth.js"
import { RepoEventHistoryQuery, type RepoEventHistoryResponse } from "../repo-events.js"

/** Opens the authenticated user-scoped feedback stream. */
export const repoStreamRoute = route("stream", {
  GET: {
    headers: BearerHeaders,
  },
})

/** Lists persisted managed pull-request events after one cursor for the current user. */
export const repoStreamHistoryRoute = route("stream/history", {
  GET: {
    headers: BearerHeaders,
    query: RepoEventHistoryQuery,
    response: $type<RepoEventHistoryResponse>(),
  },
})
