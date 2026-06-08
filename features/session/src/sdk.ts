import { defineSdkPlugin } from "@goddard-ai/sdk-plugin"

import { sessionIpcRoutes } from "./daemon-ipc.ts"

export const sessionSdkPlugin = defineSdkPlugin({
  name: "session",
  ipcRoutes: sessionIpcRoutes,
})
