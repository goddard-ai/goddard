import { $type, defineIpcRoutes, http, metadata } from "@goddard-ai/ipc"
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
    ...metadata({
      description: "Pull request operations.",
    }),
    /** Submits one pull request through the PR contract. */
    submit: http.post("submit", {
      ...metadata({
        description: "Submits one pull request through the PR contract.",
      }),
      body: SubmitPrRouteRequest,
      response: $type<SubmitPrResponse>(),
    }),
    /** Fetches one pull request by tagged id. */
    get: http.post("get", {
      ...metadata({
        description: "Fetches one pull request by tagged id.",
      }),
      body: GetPullRequestRequest,
      response: $type<GetPullRequestResponse>(),
    }),
    /** Posts one pull request reply through the PR contract. */
    reply: http.post("reply", {
      ...metadata({
        description: "Posts one pull request reply through the PR contract.",
      }),
      body: ReplyPrRouteRequest,
      response: $type<ReplyPrResponse>(),
    }),
  }),
})
