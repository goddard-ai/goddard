import { defineSdkPlugin } from "@goddard-ai/sdk-plugin"

import { inboxIpcRoutes } from "./daemon-ipc.ts"

export const inboxSdkPlugin = defineSdkPlugin({
  name: "inbox",
  ipcRoutes: inboxIpcRoutes,
})
