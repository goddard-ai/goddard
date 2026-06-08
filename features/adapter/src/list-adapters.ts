import { resolveDefaultAgent } from "@goddard-ai/config/node"
import type {
  DaemonAgentInstallService,
  DaemonAgentInstallStatus,
  DaemonInstalledAgent,
} from "@goddard-ai/daemon-plugin"
import type { AgentDistribution } from "@goddard-ai/schema/agent-distribution"
import type { AgentsConfig, StaticSessionParams } from "@goddard-ai/schema/config"

import { createConfigAdapterCatalogEntries, mergeAdapterCatalogEntries } from "./catalog.ts"
import { getAdapterInstallationStates, installAdapter, uninstallAdapter } from "./installations.ts"
import {
  AdapterCatalogEntry,
  type AdapterManagedInstall,
  type AdapterManagedInstallAgent,
  type AdapterManagedInstallState,
  type InstallAdapterRequest,
  type InstallAdapterResponse,
  type ListAdaptersRequestType,
  type ListAdaptersResponse,
  type UninstallAdapterRequest,
  type UninstallAdapterResponse,
} from "./schema.ts"

type AdapterRegistrySnapshot = Omit<
  ListAdaptersResponse,
  "adapters" | "defaultAdapterId" | "installations"
> & {
  adapters: readonly unknown[]
}

export type AdapterRegistryService = {
  listAdapters: () => Promise<AdapterRegistrySnapshot>
}

export type AdapterConfigManager = {
  getRootConfig: (cwd?: string) => Promise<{
    config: {
      agents?: AgentsConfig
      session?: StaticSessionParams
      registry?: Record<string, AgentDistribution>
    }
  }>
}

export type ListAdaptersContext = {
  registryService: AdapterRegistryService
  configProvider: AdapterConfigManager
  agentInstallService: DaemonAgentInstallService
}

function orderAdaptersByInstallationState(
  adapters: AdapterCatalogEntry[],
  installedAdapterIds: Set<string>,
) {
  return [
    ...adapters.filter((adapter) => installedAdapterIds.has(adapter.id)),
    ...adapters.filter((adapter) => !installedAdapterIds.has(adapter.id)),
  ]
}

async function readMergedAdapterCatalog(
  { registryService, configProvider }: ListAdaptersContext,
  cwd?: string,
) {
  const [registrySnapshot, resolvedConfig] = await Promise.all([
    registryService.listAdapters(),
    configProvider.getRootConfig(cwd).then((snapshot) => snapshot.config),
  ])
  const registryAdapters = registrySnapshot.adapters.map((adapter) =>
    AdapterCatalogEntry.parse(adapter),
  )
  const mergedAdapters = mergeAdapterCatalogEntries(
    registryAdapters,
    createConfigAdapterCatalogEntries(resolvedConfig?.registry),
  )

  return {
    registrySnapshot,
    resolvedConfig,
    mergedAdapters,
  }
}

/** Lists adapters from registry and config substrate using adapter feature merge semantics. */
export async function listAdapters(
  context: ListAdaptersContext,
  { cwd, includeUninstalled }: ListAdaptersRequestType,
) {
  const { registrySnapshot, resolvedConfig, mergedAdapters } = await readMergedAdapterCatalog(
    context,
    cwd,
  )
  const installations = await getAdapterInstallationStates(mergedAdapters)
  const installedAdapterIds = new Set(
    installations
      .filter((installation) => installation.installed)
      .map((installation) => installation.adapterId),
  )
  const listedAdapters = orderAdaptersByInstallationState(
    includeUninstalled
      ? mergedAdapters
      : mergedAdapters.filter((adapter) => installedAdapterIds.has(adapter.id)),
    installedAdapterIds,
  )
  const adapters = await attachManagedInstallStatus({
    adapters: listedAdapters,
    agentInstallService: context.agentInstallService,
    managedAgents: resolvedConfig?.agents?.managed,
    registry: resolvedConfig?.registry,
  })
  const defaultAgent = await resolveDefaultAgent(resolvedConfig).catch(() => null)

  return {
    ...registrySnapshot,
    adapters,
    installations,
    defaultAdapterId:
      typeof defaultAgent === "string" &&
      mergedAdapters.some((adapter) => adapter.id === defaultAgent) &&
      (includeUninstalled || installedAdapterIds.has(defaultAgent))
        ? defaultAgent
        : null,
  }
}

/** Installs one adapter from the daemon-visible catalog into the local launch set. */
export async function installCatalogAdapter(
  context: ListAdaptersContext,
  { adapterId }: InstallAdapterRequest,
): Promise<InstallAdapterResponse> {
  const { mergedAdapters } = await readMergedAdapterCatalog(context)
  const adapter = mergedAdapters.find((adapter) => adapter.id === adapterId)

  if (!adapter) {
    throw new Error(`Unknown adapter: ${adapterId}`)
  }

  return {
    adapter,
    installation: await installAdapter(adapter),
  }
}

/** Removes one adapter from the local launch set. */
export async function uninstallCatalogAdapter(
  _context: ListAdaptersContext,
  { adapterId }: UninstallAdapterRequest,
): Promise<UninstallAdapterResponse> {
  await uninstallAdapter(adapterId)

  return { adapterId }
}

async function attachManagedInstallStatus(input: {
  adapters: AdapterCatalogEntry[]
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
        } satisfies AdapterManagedInstall,
      }
    }),
  )
}

function toAdapterManagedInstallState(state: DaemonAgentInstallStatus): AdapterManagedInstallState {
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

function toAdapterManagedInstallAgent(agent: DaemonInstalledAgent): AdapterManagedInstallAgent {
  return {
    agentId: agent.agentId,
    version: agent.version,
    method: agent.method,
    installedAt: agent.installedAt,
    updatedAt: agent.updatedAt,
  }
}
