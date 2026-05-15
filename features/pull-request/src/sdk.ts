import { defineSdkPlugin } from "@goddard-ai/sdk-plugin"

export const pullRequestSdkPlugin = defineSdkPlugin({
  name: "pull-request",
  create() {
    return {
      pullRequest: {},
    }
  },
})
