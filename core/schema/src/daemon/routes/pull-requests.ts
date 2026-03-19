import { $type, route } from "rouzer"
import { BearerHeaders } from "../../common/auth.js"
import { type ReplyPrDaemonResponse, type SubmitPrDaemonResponse } from "../../daemon.js"
import { ReplyPrDaemonRequest, SubmitPrDaemonRequest } from "../pull-requests.js"

/** Creates a pull request through the daemon. */
export const prSubmitRoute = route("pr/submit", {
  POST: {
    headers: BearerHeaders,
    body: SubmitPrDaemonRequest,
    response: $type<SubmitPrDaemonResponse>(),
  },
})

/** Posts a pull-request reply through the daemon. */
export const prReplyRoute = route("pr/reply", {
  POST: {
    headers: BearerHeaders,
    body: ReplyPrDaemonRequest,
    response: $type<ReplyPrDaemonResponse>(),
  },
})
