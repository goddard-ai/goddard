import { $type, route } from "rouzer"
import { BearerHeaders } from "../../common/auth.js"
import {
  CreatePrInput,
  ManagedPrQuery,
  ReplyPrInput,
  type PullRequestRecord,
} from "../pull-requests.js"

/** Creates a managed pull request through the backend. */
export const prCreateRoute = route("pr/create", {
  POST: {
    headers: BearerHeaders,
    body: CreatePrInput,
    response: $type<PullRequestRecord>(),
  },
})

/** Posts a managed pull-request reply through the backend. */
export const prReplyRoute = route("pr/reply", {
  POST: {
    headers: BearerHeaders,
    body: ReplyPrInput,
    response: $type<{ success: boolean }>(),
  },
})

/** Reports whether a pull request is managed by the authenticated user. */
export const prManagedRoute = route("pr/managed", {
  GET: {
    headers: BearerHeaders,
    query: ManagedPrQuery,
    response: $type<{ managed: boolean }>(),
  },
})
