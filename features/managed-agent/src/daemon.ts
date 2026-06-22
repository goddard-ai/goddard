import { definePlugin } from "@goddard-ai/daemon-plugin"
import type { AgentDistribution } from "@goddard-ai/schema/agent-distribution"
import { createAcpRegistryService } from "acp-client/node"

import { managedAgentIpcRoutes } from "./daemon-ipc.ts"
import {
  installCatalogManagedAgent,
  listManagedAgents,
  uninstallCatalogManagedAgent,
} from "./list-managed-agents.ts"

export type ManagedAgentService = {
  readonly resolveAgent: (input: {
    readonly agent: string | AgentDistribution
    readonly registry?: Record<string, AgentDistribution>
  }) => Promise<AgentDistribution>
}

export const managedAgentPlugin = definePlugin({
  name: "managed-agent",
  ipcRoutes: managedAgentIpcRoutes,
  setup({ agentInstallService, configProvider }) {
    const registryService = createAcpRegistryService()
    const managedAgentContext = {
      agentInstallService,
      configProvider,
      registryService,
    }
    const managedAgentService: ManagedAgentService = {
      async resolveAgent({ agent, registry }) {
        if (typeof agent !== "string") {
          return agent
        }

        const configuredAgent = registry?.[agent]
        if (configuredAgent) {
          return configuredAgent
        }

        const registryEntry = await registryService.getAdapter(agent)
        if (!registryEntry.adapter) {
          throw new Error(`ACP agent not found: ${agent}`)
        }

        return registryEntry.adapter
      },
    }

    return {
      provides: {
        managedAgent: managedAgentService,
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
