import { defineSdkPlugin } from "@goddard-ai/sdk-plugin"

import {
  pullRequestIpcRoutes,
  type ReplyPrRouteRequest,
  type SubmitPrRouteRequest,
} from "./daemon-ipc.ts"
import type { GetPullRequestRequest } from "./schema.ts"

export const pullRequestSdkPlugin = defineSdkPlugin({
  name: "pull-request",
  ipcRoutes: pullRequestIpcRoutes,
  create({ client }) {
    return {
      pr: {
        /** Submits one pull request through the daemon PR contract. */
        submit: (input: SubmitPrRouteRequest) => client.send("pr.submit", input),

        /** Fetches one daemon-managed pull request by tagged id. */
        get: (input: GetPullRequestRequest) => client.send("pr.get", input),

        /** Posts one pull request reply through the daemon PR contract. */
        reply: (input: ReplyPrRouteRequest) => client.send("pr.reply", input),
      },
    }
  },
})
