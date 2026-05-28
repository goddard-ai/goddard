import { defineSdkPlugin } from "@goddard-ai/sdk-plugin"

import { authIpcRoutes } from "./daemon-ipc.ts"

export const authSdkPlugin = defineSdkPlugin({
  name: "auth",
  ipcRoutes: authIpcRoutes,
})
