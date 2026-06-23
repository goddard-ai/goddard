import { defineSdkPlugin } from "@goddard-ai/sdk-plugin"

import { agentIpcRoutes } from "./daemon-ipc.ts"

export const agentSdkPlugin = defineSdkPlugin({
  name: "agent",
  ipcRoutes: agentIpcRoutes,
})
