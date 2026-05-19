import { defineRequest, defineSdkPlugin } from "@goddard-ai/sdk-plugin"

import { pullRequestIpcSchema } from "./daemon-ipc.ts"

export const pullRequestSdkPlugin = defineSdkPlugin({
  name: "pull-request",
  ipc: pullRequestIpcSchema,
  create({ client }) {
    return {
      pr: {
        /** Submits one pull request through the daemon PR contract. */
        submit: defineRequest(client, "pr.submit"),

        /** Fetches one daemon-managed pull request by tagged id. */
        get: defineRequest(client, "pr.get"),

        /** Posts one pull request reply through the daemon PR contract. */
        reply: defineRequest(client, "pr.reply"),
      },
    }
  },
})
