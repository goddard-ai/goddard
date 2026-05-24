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
  wrap({ client }) {
    return {
      pr: {
        /** Submits one pull request through the daemon PR contract. */
        submit: (input: SubmitPrRouteRequest) => client.pr.submit({ body: input }),

        /** Fetches one daemon-managed pull request by tagged id. */
        get: (input: GetPullRequestRequest) => client.pr.get({ body: input }),

        /** Posts one pull request reply through the daemon PR contract. */
        reply: (input: ReplyPrRouteRequest) => client.pr.reply({ body: input }),
      },
    }
  },
})
