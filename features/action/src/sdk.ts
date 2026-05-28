import { defineSdkPlugin } from "@goddard-ai/sdk-plugin"

import { actionIpcRoutes } from "./daemon-ipc.ts"

export const actionSdkPlugin = defineSdkPlugin({
  name: "action",
  ipcRoutes: actionIpcRoutes,
})
