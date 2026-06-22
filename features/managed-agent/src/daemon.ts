import { definePlugin, type DaemonMetadataStore } from "@goddard-ai/daemon-plugin"
import { createAcpRegistryService } from "acp-client/node"

import { managedAgentIpcRoutes } from "./daemon-ipc.ts"
import {
  createManagedAgentInstallService,
  type ManagedAgentInstallService,
  type ManagedAgentUsageState,
} from "./daemon/install-service.ts"
import {
  installCatalogManagedAgent,
  listManagedAgents,
  uninstallCatalogManagedAgent,
} from "./list-managed-agents.ts"

export type ManagedAgentService = ManagedAgentInstallService

export const managedAgentPlugin = definePlugin({
  name: "managed-agent",
  ipcRoutes: managedAgentIpcRoutes,
  setup({ configProvider, metadataStore }) {
    const registryService = createAcpRegistryService()
    const managedAgent = createManagedAgentInstallService({
      registryService,
      usageStore: createManagedAgentUsageStore(metadataStore),
    })
    const managedAgentContext = {
      agentInstallService: managedAgent,
      configProvider,
      registryService,
    }

    return {
      provides: {
        managedAgent,
      },
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

function createManagedAgentUsageStore(metadataStore: DaemonMetadataStore) {
  return {
    get: () => (metadataStore.get("managedAgentUsage") as ManagedAgentUsageState | undefined) ?? {},
    set: (state: ManagedAgentUsageState) => {
      metadataStore.set("managedAgentUsage", state)
    },
  }
}
