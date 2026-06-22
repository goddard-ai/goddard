import { dirname, join, resolve } from "node:path"
import type { ACPRegistryService } from "@goddard-ai/daemon-plugin"
import { getDatabasePath } from "@goddard-ai/paths/node"
import type { AgentDistribution } from "@goddard-ai/schema/agent-distribution"
import {
  createAcpRegistryService,
  ensureAgentInstalled,
  getInstalledAgent,
  listInstalledAgents,
  resolveInstalledAgentProcessSpec,
  updateAgent,
  type AgentInstallOptions,
} from "acp-client/node"

export type ManagedAgentProcessSpec = {
  readonly cmd: string
  readonly args: readonly string[]
  readonly env?: Record<string, string>
}

export type InstalledManagedAgent = {
  readonly agentId: string
  readonly version: string
  readonly distributionHash: string
  readonly method: "binary" | "npx" | "uvx"
  readonly platform?: string
  readonly installDir: string
  readonly installedAt: string
  readonly updatedAt: string
}

export type ManagedAgentInstallStatus =
  | { readonly status: "missing" }
  | { readonly status: "installed"; readonly agent: InstalledManagedAgent }
  | {
      readonly status: "failed"
      readonly lastError: string
      readonly checkedAt: string
      readonly agent?: InstalledManagedAgent
    }

export type ManagedAgentInstallResult = {
  readonly agent: InstalledManagedAgent
  readonly installed: boolean
  readonly updated: boolean
}

export type ManagedAgentUpdateResult = {
  readonly agent: InstalledManagedAgent
  readonly checkedAt: string
  readonly updated: boolean
  readonly previous?: InstalledManagedAgent
}

export type ManagedAgentInput = {
  readonly agent: string | AgentDistribution
  readonly registry?: Record<string, AgentDistribution>
}

export type ManagedAgentInstallService = {
  readonly cacheDir: string
  readonly resolveAgent: (input: ManagedAgentInput) => Promise<AgentDistribution>
  readonly getInstalledAgent: (input: ManagedAgentInput) => Promise<ManagedAgentInstallStatus>
  readonly listInstalledAgents: () => Promise<readonly InstalledManagedAgent[]>
  readonly ensureAgentInstalled: (input: ManagedAgentInput) => Promise<ManagedAgentInstallResult>
  readonly updateAgent: (input: ManagedAgentInput) => Promise<ManagedAgentUpdateResult>
  readonly resolveInstalledAgentProcessSpec: (
    input: ManagedAgentInput & { readonly installIfMissing?: boolean },
  ) => Promise<ManagedAgentProcessSpec>
}

export type ManagedAgentUsageState = Record<
  string,
  {
    readonly lastUsedAt: string
  }
>

export type ManagedAgentUsageStore = {
  readonly get: () => ManagedAgentUsageState | undefined
  readonly set: (state: ManagedAgentUsageState) => void
}

type AcpClientManagedInstallApi = {
  getInstalledAgent: typeof getInstalledAgent
  listInstalledAgents: typeof listInstalledAgents
  ensureAgentInstalled: typeof ensureAgentInstalled
  updateAgent: typeof updateAgent
  resolveInstalledAgentProcessSpec: typeof resolveInstalledAgentProcessSpec
}

type ManagedAgentInstallServiceOptions = {
  registryService?: ACPRegistryService
  cacheDir?: string
  now?: () => number
  usageStore?: ManagedAgentUsageStore
  managedInstallApi?: AcpClientManagedInstallApi
}

const defaultManagedInstallApi = {
  getInstalledAgent,
  listInstalledAgents,
  ensureAgentInstalled,
  updateAgent,
  resolveInstalledAgentProcessSpec,
} satisfies AcpClientManagedInstallApi

// acp-client uses milliseconds, so the three-month launch fallback is a 90-day window.
const MANAGED_AGENT_LAUNCH_FALLBACK_MAX_AGE_MS = 90 * 24 * 60 * 60 * 1000

/** Returns the profile-scoped acp-client cache root used for managed agent installs. */
export function getDaemonAgentInstallCacheDir() {
  return join(dirname(getDatabasePath()), "acp-client")
}

/** Creates the managed-agent policy wrapper around acp-client install operations. */
export function createManagedAgentInstallService(
  options: ManagedAgentInstallServiceOptions,
): ManagedAgentInstallService {
  const cacheDir = resolve(options.cacheDir ?? getDaemonAgentInstallCacheDir())
  const managedInstallApi = options.managedInstallApi ?? defaultManagedInstallApi
  const registryService = options.registryService ?? createAcpRegistryService()

  function installOptions(extra: AgentInstallOptions = {}): AgentInstallOptions {
    return {
      cacheDir,
      now: options.now,
      ...extra,
    }
  }

  async function resolveAgent(input: ManagedAgentInput): Promise<AgentDistribution> {
    if (typeof input.agent !== "string") {
      return input.agent
    }

    const configuredAgent = input.registry?.[input.agent]
    if (configuredAgent) {
      return configuredAgent
    }

    const registryEntry = await registryService.getAdapter(input.agent)
    if (!registryEntry.adapter) {
      throw new Error(`Managed ACP agent not found: ${input.agent}`)
    }

    return registryEntry.adapter
  }

  return {
    cacheDir,

    resolveAgent,

    async getInstalledAgent(input) {
      const agent = await resolveAgent(input)
      return managedInstallApi.getInstalledAgent(agent, installOptions())
    },

    listInstalledAgents() {
      return managedInstallApi.listInstalledAgents(installOptions())
    },

    async ensureAgentInstalled(input) {
      const agent = await resolveAgent(input)
      return managedInstallApi.ensureAgentInstalled(agent, installOptions())
    },

    async updateAgent(input) {
      const agent = await resolveAgent(input)
      return managedInstallApi.updateAgent(agent, installOptions())
    },

    async resolveInstalledAgentProcessSpec(input) {
      const agent = await resolveAgent(input)
      const processSpec = await managedInstallApi.resolveInstalledAgentProcessSpec(
        agent,
        installOptions({
          installIfMissing: input.installIfMissing,
          maxInstalledAgeMs: MANAGED_AGENT_LAUNCH_FALLBACK_MAX_AGE_MS,
        }),
      )
      if (options.usageStore) {
        recordManagedAgentUsed(options.usageStore, agent.id, readNowIso(options.now))
      }
      return processSpec
    },
  }
}

function recordManagedAgentUsed(
  usageStore: ManagedAgentUsageStore,
  agentId: string,
  lastUsedAt: string,
) {
  usageStore.set({
    ...usageStore.get(),
    [agentId]: { lastUsedAt },
  })
}

function readNowIso(now?: () => number) {
  return new Date(now?.() ?? Date.now()).toISOString()
}
