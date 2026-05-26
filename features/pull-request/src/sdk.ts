import { defineSdkPlugin } from "@goddard-ai/sdk-plugin"

import { pullRequestIpcRoutes } from "./daemon-ipc.ts"

export const pullRequestSdkPlugin = defineSdkPlugin({
  name: "pull-request",
  ipcRoutes: pullRequestIpcRoutes,
})
