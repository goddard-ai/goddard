import { definePlugin, type DaemonMetadataStore } from "@goddard-ai/daemon-plugin"
import { createAcpRegistryService } from "acp-client/node"

import { managedAgentIpcRoutes } from "./daemon-ipc.ts"
import {
  createManagedAgentInstallService,
  type ManagedAgentInstallService,
} from "./daemon/install-service.ts"
import {
  resolveManagedAgentLaunchProcessSpec,
  type ResolveManagedAgentLaunchProcessSpecInput,
} from "./daemon/launch-process.ts"
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

export type AgentService = ManagedAgentInstallService & {
  readonly resolveLaunchProcessSpec: (
    input: ResolveManagedAgentLaunchProcessSpecInput,
  ) => ReturnType<typeof resolveManagedAgentLaunchProcessSpec>
}

export const agentPlugin = definePlugin({
  name: "agent",
  ipcRoutes: managedAgentIpcRoutes,
  setup({ configProvider, log, metadataStore }) {
    const registryService = createAcpRegistryService()
    const usageStore = createManagedAgentUsageStore(metadataStore)
    const managedAgentInstallService = createManagedAgentInstallService({
      registryService,
      usageStore,
    })
    const agent: AgentService = {
      ...managedAgentInstallService,
      resolveLaunchProcessSpec: (input) =>
        resolveManagedAgentLaunchProcessSpec(managedAgentInstallService, input),
    }
    const updateScheduler = createManagedAgentUpdateScheduler({
      configProvider,
      agentInstallService: agent,
      updateCheckStore: createManagedAgentUpdateCheckStore(metadataStore),
      usageStore,
      logger: log.createLogger(),
    })
    const agentContext = {
      agentInstallService: agent,
      configProvider,
      registryService,
    }
    updateScheduler.start()

    return {
      provides: {
        agent,
      },
      ipcHandlers: {
        managedAgent: {
          list: async ({ body }) => listManagedAgents(agentContext, body),
          install: async ({ body }) => installCatalogManagedAgent(agentContext, body),
          uninstall: async ({ body }) => uninstallCatalogManagedAgent(agentContext, body),
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
