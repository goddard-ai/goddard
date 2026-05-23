import { $type, defineIpcRoutes, defineIpcSchema, http } from "@goddard-ai/ipc"
import { z } from "zod"

import {
  GetPullRequestRequest,
  ReplyPrRequest,
  SubmitPrRequest,
  type GetPullRequestResponse,
  type ReplyPrResponse,
  type SubmitPrResponse,
} from "./schema.ts"

export const pullRequestIpcSchema = defineIpcSchema({
  requests: {
    "pr.submit": {
      payload: SubmitPrRequest.extend({
        token: z.string(),
      }),
      response: $type<SubmitPrResponse>(),
    },
    "pr.get": {
      payload: GetPullRequestRequest,
      response: $type<GetPullRequestResponse>(),
    },
    "pr.reply": {
      payload: ReplyPrRequest.extend({
        token: z.string(),
      }),
      response: $type<ReplyPrResponse>(),
    },
  },
  streams: {},
})

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
    submit: http.post("submit", {
      body: SubmitPrRouteRequest,
      response: $type<SubmitPrResponse>(),
    }),
    get: http.post("get", {
      body: GetPullRequestRequest,
      response: $type<GetPullRequestResponse>(),
    }),
    reply: http.post("reply", {
      body: ReplyPrRouteRequest,
      response: $type<ReplyPrResponse>(),
    }),
  }),
})
