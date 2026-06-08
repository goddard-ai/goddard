import { defineSdkPlugin } from "@goddard-ai/sdk-plugin"

import { workforceIpcRoutes } from "./daemon-ipc.ts"

export const workforceSdkPlugin = defineSdkPlugin({
  name: "workforce",
  ipcRoutes: workforceIpcRoutes,
})
