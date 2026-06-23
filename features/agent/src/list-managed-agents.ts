import { resolveDefaultAgent } from "@goddard-ai/config/node"
import type { AgentDistribution } from "@goddard-ai/schema/agent-distribution"
import type { AgentsConfig, StaticSessionParams } from "@goddard-ai/schema/config"
import { omit } from "radashi"

import {
  createConfigManagedAgentCatalogEntries,
  mergeManagedAgentCatalogEntries,
} from "./catalog.ts"
import type {
  InstalledManagedAgent,
  ManagedAgentInstallService,
  ManagedAgentInstallStatus,
} from "./daemon/install-service.ts"
import {
  getAgentInstallationStates,
  installManagedAgent,
  uninstallManagedAgent,
} from "./installations.ts"
import {
  AgentCatalogEntry,
  type InstallAgentRequest,
  type InstallAgentResponse,
  type ListAgentsRequestType,
  type ListAgentsResponse,
  type ManagedAgentInstall,
  type ManagedAgentInstallAgent,
  type ManagedAgentInstallState,
  type UninstallAgentRequest,
  type UninstallAgentResponse,
} from "./schema.ts"

type ManagedAgentRegistrySnapshot = Omit<
  ListAgentsResponse,
  "agents" | "defaultAgentId" | "installations"
> & {
  adapters: readonly unknown[]
}

export type ManagedAgentRegistryService = {
  listAdapters: () => Promise<ManagedAgentRegistrySnapshot>
}

export type ManagedAgentConfigManager = {
  getRootConfig: (cwd?: string) => Promise<{
    config: {
      agents?: AgentsConfig
      session?: StaticSessionParams
      registry?: Record<string, AgentDistribution>
    }
  }>
}

export type ListAgentsContext = {
  registryService: ManagedAgentRegistryService
  configProvider: ManagedAgentConfigManager
  agentInstallService: ManagedAgentInstallService
}

function orderAdaptersByInstallationState(
  adapters: AgentCatalogEntry[],
  installedAdapterIds: Set<string>,
) {
  return [
    ...adapters.filter((adapter) => installedAdapterIds.has(adapter.id)),
    ...adapters.filter((adapter) => !installedAdapterIds.has(adapter.id)),
  ]
}

function isManagedAgent(adapter: AgentCatalogEntry, agents?: AgentsConfig["managed"]) {
  return agents?.[adapter.id] !== undefined
}

async function readMergedAdapterCatalog(
  { registryService, configProvider }: ListAgentsContext,
  cwd?: string,
) {
  const [registrySnapshot, resolvedConfig] = await Promise.all([
    registryService.listAdapters(),
    configProvider.getRootConfig(cwd).then((snapshot) => snapshot.config),
  ])
  const registryAdapters = registrySnapshot.adapters.map((adapter) =>
    AgentCatalogEntry.parse(adapter),
  )
  const mergedAdapters = mergeManagedAgentCatalogEntries(
    registryAdapters,
    createConfigManagedAgentCatalogEntries(resolvedConfig?.registry),
  )

  return {
    registrySnapshot,
    resolvedConfig,
    mergedAdapters,
  }
}

/** Lists managed agents from registry and config substrate using managed-agent merge semantics. */
export async function listManagedAgents(
  context: ListAgentsContext,
  { cwd, includeUninstalled }: ListAgentsRequestType,
) {
  const { registrySnapshot, resolvedConfig, mergedAdapters } = await readMergedAdapterCatalog(
    context,
    cwd,
  )
  const installations = await getAgentInstallationStates(mergedAdapters)
  const installedAdapterIds = new Set(
    installations
      .filter((installation) => installation.installed)
      .map((installation) => installation.agentId),
  )
  const listedAdapters = orderAdaptersByInstallationState(
    includeUninstalled
      ? mergedAdapters
      : mergedAdapters.filter(
          (adapter) =>
            installedAdapterIds.has(adapter.id) ||
            isManagedAgent(adapter, resolvedConfig?.agents?.managed),
        ),
    installedAdapterIds,
  )
  const agents = await attachManagedInstallStatus({
    adapters: listedAdapters,
    agentInstallService: context.agentInstallService,
    agents: resolvedConfig?.agents?.managed,
    registry: resolvedConfig?.registry,
  })
  const defaultAgent = await resolveDefaultAgent(resolvedConfig).catch(() => null)

  return {
    ...registrySnapshot,
    agents,
    installations,
    defaultAgentId:
      typeof defaultAgent === "string" &&
      mergedAdapters.some((adapter) => adapter.id === defaultAgent) &&
      (includeUninstalled ||
        installedAdapterIds.has(defaultAgent) ||
        resolvedConfig?.agents?.managed?.[defaultAgent] !== undefined)
        ? defaultAgent
        : null,
  }
}

/** Installs one managed agent from the daemon-visible catalog into the local launch set. */
export async function installCatalogManagedAgent(
  context: ListAgentsContext,
  { agentId }: InstallAgentRequest,
): Promise<InstallAgentResponse> {
  const { mergedAdapters } = await readMergedAdapterCatalog(context)
  const adapter = mergedAdapters.find((adapter) => adapter.id === agentId)

  if (!adapter) {
    throw new Error(`Unknown managed agent: ${agentId}`)
  }

  return {
    agent: adapter,
    installation: await installManagedAgent(adapter),
  }
}

/** Removes one managed agent from the local launch set. */
export async function uninstallCatalogManagedAgent(
  _context: ListAgentsContext,
  { agentId }: UninstallAgentRequest,
): Promise<UninstallAgentResponse> {
  await uninstallManagedAgent(agentId)

  return { agentId }
}

async function attachManagedInstallStatus(input: {
  adapters: AgentCatalogEntry[]
  agentInstallService: ManagedAgentInstallService
  agents?: AgentsConfig["managed"]
  registry?: Record<string, AgentDistribution>
}) {
  if (!input.agents) {
    return input.adapters
  }

  return Promise.all(
    input.adapters.map(async (adapter) => {
      const managedAgent = input.agents?.[adapter.id]
      if (!managedAgent) {
        return adapter
      }

      const state = await input.agentInstallService.getInstalledAgent({
        agent: adapter.id,
        registry: input.registry,
      })

      return {
        ...adapter,
        managedInstall: {
          managed: true,
          install: managedAgent.install,
          update: managedAgent.update,
          state: toAdapterManagedInstallState(state),
        } satisfies ManagedAgentInstall,
      }
    }),
  )
}

function toAdapterManagedInstallState(state: ManagedAgentInstallStatus): ManagedAgentInstallState {
  if (state.status === "missing") {
    return { status: "missing" }
  }

  if (state.status === "installed") {
    return {
      status: "installed",
      agent: toAdapterManagedInstallAgent(state.agent),
    }
  }

  return {
    status: "failed",
    lastError: state.lastError,
    checkedAt: state.checkedAt,
    agent: state.agent ? toAdapterManagedInstallAgent(state.agent) : undefined,
  }
}

function toAdapterManagedInstallAgent(agent: InstalledManagedAgent): ManagedAgentInstallAgent {
  return omit(agent, ["distributionHash", "platform", "installDir"])
}
