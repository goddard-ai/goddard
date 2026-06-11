import { defineSdkPlugin } from "@goddard-ai/sdk-plugin"

import { fileSearchIpcRoutes } from "./daemon-ipc.ts"

export const fileSearchSdkPlugin = defineSdkPlugin({
  name: "file-search",
  ipcRoutes: fileSearchIpcRoutes,
})
