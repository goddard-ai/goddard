import { defineSdkPlugin } from "@goddard-ai/sdk-plugin"

import { loopIpcRoutes } from "./daemon-ipc.ts"

export const loopSdkPlugin = defineSdkPlugin({
  name: "loop",
  ipcRoutes: loopIpcRoutes,
})
