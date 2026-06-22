import { definePlugin, type DaemonMetadataStore } from "@goddard-ai/daemon-plugin"
import { createAcpRegistryService } from "acp-client/node"

import { managedAgentIpcRoutes } from "./daemon-ipc.ts"
import {
  createManagedAgentInstallService,
  type ManagedAgentInstallService,
} from "./daemon/install-service.ts"
import {
  createManagedAgentUpdateScheduler,
  type ManagedAgentUpdateCheckState,
} from "./daemon/update-scheduler.ts"
import type { ManagedAgentUsageState } from "./daemon/usage-store.ts"
import {
  installCatalogManagedAgent,
  listManagedAgents,
  uninstallCatalogManagedAgent,
} from "./list-managed-agents.ts"

export type ManagedAgentService = ManagedAgentInstallService

export const managedAgentPlugin = definePlugin({
  name: "managed-agent",
  ipcRoutes: managedAgentIpcRoutes,
  setup({ configProvider, log, metadataStore }) {
    const registryService = createAcpRegistryService()
    const usageStore = createManagedAgentUsageStore(metadataStore)
    const managedAgent = createManagedAgentInstallService({
      registryService,
      usageStore,
    })
    const updateScheduler = createManagedAgentUpdateScheduler({
      configProvider,
      agentInstallService: managedAgent,
      updateCheckStore: createManagedAgentUpdateCheckStore(metadataStore),
      usageStore,
      logger: log.createLogger(),
    })
    const managedAgentContext = {
      agentInstallService: managedAgent,
      configProvider,
      registryService,
    }
    updateScheduler.start()

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
      close: () => {
        updateScheduler.close()
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

function createManagedAgentUpdateCheckStore(metadataStore: DaemonMetadataStore) {
  return {
    get: () =>
      (metadataStore.get("managedAgentUpdateChecks") as ManagedAgentUpdateCheckState | undefined) ??
      {},
    set: (state: ManagedAgentUpdateCheckState) => {
      metadataStore.set("managedAgentUpdateChecks", state)
    },
  }
}
