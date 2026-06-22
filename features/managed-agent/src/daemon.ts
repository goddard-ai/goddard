import { definePlugin } from "@goddard-ai/daemon-plugin"

import { managedAgentIpcRoutes } from "./daemon-ipc.ts"

export const managedAgentPlugin = definePlugin({
  name: "managed-agent",
  ipcRoutes: managedAgentIpcRoutes,
  setup() {
    return {}
  },
})
