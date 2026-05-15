import { defineSdkPlugin } from "@goddard-ai/sdk-plugin"

import { pullRequestIpcSchema } from "./daemon-ipc.ts"

export const pullRequestSdkPlugin = defineSdkPlugin({
  name: "pull-request",
  ipc: pullRequestIpcSchema,
  create() {
    return {
      pullRequest: {},
    }
  },
})
