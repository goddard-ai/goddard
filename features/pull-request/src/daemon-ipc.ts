import { $type, defineIpcSchema } from "@goddard-ai/ipc"
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
