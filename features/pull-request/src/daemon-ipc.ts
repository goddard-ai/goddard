import { $type, defineIpcRoutes, http, ipcMetadata } from "@goddard-ai/ipc"
import { z } from "zod"

import {
  GetPullRequestRequest,
  ReplyPrRequest,
  SubmitPrRequest,
  type GetPullRequestResponse,
  type ReplyPrResponse,
  type SubmitPrResponse,
} from "./schema.ts"

export const SubmitPrRouteRequest = SubmitPrRequest.extend({
  token: z.string(),
})

export type SubmitPrRouteRequest = z.infer<typeof SubmitPrRouteRequest>

export const ReplyPrRouteRequest = ReplyPrRequest.extend({
  token: z.string(),
})

export type ReplyPrRouteRequest = z.infer<typeof ReplyPrRouteRequest>

export const pullRequestIpcRoutes = defineIpcRoutes({
  pr: http.resource("pr", {
    ...ipcMetadata({
      description: "Daemon-managed pull request operations.",
    }),
    /** Submits one pull request through the daemon PR contract. */
    submit: http.post("submit", {
      ...ipcMetadata({
        description: "Submits one pull request through the daemon PR contract.",
      }),
      body: SubmitPrRouteRequest,
      response: $type<SubmitPrResponse>(),
    }),
    /** Fetches one daemon-managed pull request by tagged id. */
    get: http.post("get", {
      ...ipcMetadata({
        description: "Fetches one daemon-managed pull request by tagged id.",
      }),
      body: GetPullRequestRequest,
      response: $type<GetPullRequestResponse>(),
    }),
    /** Posts one pull request reply through the daemon PR contract. */
    reply: http.post("reply", {
      ...ipcMetadata({
        description: "Posts one pull request reply through the daemon PR contract.",
      }),
      body: ReplyPrRouteRequest,
      response: $type<ReplyPrResponse>(),
    }),
  }),
})
