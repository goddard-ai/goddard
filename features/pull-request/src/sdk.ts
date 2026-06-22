import { defineSdkPlugin } from "@goddard-ai/sdk-plugin"

import { pullRequestIpcRoutes } from "./daemon-ipc.ts"
import { pullRequestEvents } from "./events.ts"

export const pullRequestSdkPlugin = defineSdkPlugin({
  name: "pull-request",
  ipcRoutes: pullRequestIpcRoutes,
  events: pullRequestEvents,
})
