import { definePlugin } from "@goddard-ai/daemon-plugin"

import { managedAgentIpcRoutes } from "./daemon-ipc.ts"
import {
  installCatalogManagedAgent,
  listManagedAgents,
  uninstallCatalogManagedAgent,
} from "./list-managed-agents.ts"

export const managedAgentPlugin = definePlugin({
  name: "managed-agent",
  ipcRoutes: managedAgentIpcRoutes,
  setup({ agentInstallService, configProvider, registryService }) {
    const managedAgentContext = {
      agentInstallService,
      configProvider,
      registryService,
    }

    return {
      ipcHandlers: {
        managedAgent: {
          list: async ({ body }) => listManagedAgents(managedAgentContext, body),
          install: async ({ body }) => installCatalogManagedAgent(managedAgentContext, body),
          uninstall: async ({ body }) => uninstallCatalogManagedAgent(managedAgentContext, body),
        },
      },
    }
  },
})
