import { defineSdkPlugin } from "@goddard-ai/sdk-plugin"

import { adapterIpcRoutes } from "./daemon-ipc.ts"

export const adapterSdkPlugin = defineSdkPlugin({
  name: "adapter",
  ipcRoutes: adapterIpcRoutes,
})
