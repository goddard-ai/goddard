import { resolveDefaultAgent } from "@goddard-ai/config/node"
import type {
  DaemonAgentInstallService,
  DaemonAgentInstallStatus,
  DaemonInstalledAgent,
} from "@goddard-ai/daemon-plugin"
import type { AgentDistribution } from "@goddard-ai/schema/agent-distribution"
import type { AgentsConfig, StaticSessionParams } from "@goddard-ai/schema/config"
import { omit } from "radashi"

import {
  createConfigManagedAgentCatalogEntries,
  mergeManagedAgentCatalogEntries,
} from "./catalog.ts"
import {
  getManagedAgentInstallationStates,
  installManagedAgent,
  uninstallManagedAgent,
} from "./installations.ts"
import {
  ManagedAgentCatalogEntry,
  type InstallManagedAgentRequest,
  type InstallManagedAgentResponse,
  type ListManagedAgentsRequestType,
  type ListManagedAgentsResponse,
  type ManagedAgentInstall,
  type ManagedAgentInstallAgent,
  type ManagedAgentInstallState,
  type UninstallManagedAgentRequest,
  type UninstallManagedAgentResponse,
} from "./schema.ts"

type ManagedAgentRegistrySnapshot = Omit<
  ListManagedAgentsResponse,
  "managedAgents" | "defaultManagedAgentId" | "installations"
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

export type ListManagedAgentsContext = {
  registryService: ManagedAgentRegistryService
  configProvider: ManagedAgentConfigManager
  agentInstallService: DaemonAgentInstallService
}

function orderAdaptersByInstallationState(
  adapters: ManagedAgentCatalogEntry[],
  installedAdapterIds: Set<string>,
) {
  return [
    ...adapters.filter((adapter) => installedAdapterIds.has(adapter.id)),
    ...adapters.filter((adapter) => !installedAdapterIds.has(adapter.id)),
  ]
}

function isManagedAgent(
  adapter: ManagedAgentCatalogEntry,
  managedAgents?: AgentsConfig["managed"],
) {
  return managedAgents?.[adapter.id] !== undefined
}

async function readMergedAdapterCatalog(
  { registryService, configProvider }: ListManagedAgentsContext,
  cwd?: string,
) {
  const [registrySnapshot, resolvedConfig] = await Promise.all([
    registryService.listAdapters(),
    configProvider.getRootConfig(cwd).then((snapshot) => snapshot.config),
  ])
  const registryAdapters = registrySnapshot.adapters.map((adapter) =>
    ManagedAgentCatalogEntry.parse(adapter),
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
  context: ListManagedAgentsContext,
  { cwd, includeUninstalled }: ListManagedAgentsRequestType,
) {
  const { registrySnapshot, resolvedConfig, mergedAdapters } = await readMergedAdapterCatalog(
    context,
    cwd,
  )
  const installations = await getManagedAgentInstallationStates(mergedAdapters)
  const installedAdapterIds = new Set(
    installations
      .filter((installation) => installation.installed)
      .map((installation) => installation.managedAgentId),
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
  const managedAgents = await attachManagedInstallStatus({
    adapters: listedAdapters,
    agentInstallService: context.agentInstallService,
    managedAgents: resolvedConfig?.agents?.managed,
    registry: resolvedConfig?.registry,
  })
  const defaultAgent = await resolveDefaultAgent(resolvedConfig).catch(() => null)

  return {
    ...registrySnapshot,
    managedAgents,
    installations,
    defaultManagedAgentId:
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
  context: ListManagedAgentsContext,
  { managedAgentId }: InstallManagedAgentRequest,
): Promise<InstallManagedAgentResponse> {
  const { mergedAdapters } = await readMergedAdapterCatalog(context)
  const adapter = mergedAdapters.find((adapter) => adapter.id === managedAgentId)

  if (!adapter) {
    throw new Error(`Unknown managed agent: ${managedAgentId}`)
  }

  return {
    managedAgent: adapter,
    installation: await installManagedAgent(adapter),
  }
}

/** Removes one managed agent from the local launch set. */
export async function uninstallCatalogManagedAgent(
  _context: ListManagedAgentsContext,
  { managedAgentId }: UninstallManagedAgentRequest,
): Promise<UninstallManagedAgentResponse> {
  await uninstallManagedAgent(managedAgentId)

  return { managedAgentId }
}

async function attachManagedInstallStatus(input: {
  adapters: ManagedAgentCatalogEntry[]
  agentInstallService: DaemonAgentInstallService
  managedAgents?: AgentsConfig["managed"]
  registry?: Record<string, AgentDistribution>
}) {
  if (!input.managedAgents) {
    return input.adapters
  }

  return Promise.all(
    input.adapters.map(async (adapter) => {
      const managedAgent = input.managedAgents?.[adapter.id]
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

function toAdapterManagedInstallState(state: DaemonAgentInstallStatus): ManagedAgentInstallState {
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

function toAdapterManagedInstallAgent(agent: DaemonInstalledAgent): ManagedAgentInstallAgent {
  return omit(agent, ["distributionHash", "platform", "installDir"])
}
