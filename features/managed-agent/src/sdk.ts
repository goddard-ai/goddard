import { defineSdkPlugin } from "@goddard-ai/sdk-plugin"

import { managedAgentIpcRoutes } from "./daemon-ipc.ts"

export const managedAgentSdkPlugin = defineSdkPlugin({
  name: "managed-agent",
  ipcRoutes: managedAgentIpcRoutes,
})
