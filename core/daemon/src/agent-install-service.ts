import { dirname, join, resolve } from "node:path"
import type {
  ACPRegistryService,
  DaemonAgentInstallService,
  DaemonManagedAgentInput,
} from "@goddard-ai/daemon-plugin"
import { getDatabasePath } from "@goddard-ai/paths/node"
import type { AgentDistribution } from "@goddard-ai/schema/agent-distribution"
import {
  ensureAgentInstalled,
  getInstalledAgent,
  listInstalledAgents,
  resolveInstalledAgentProcessSpec,
  updateAgent,
  type AgentInstallOptions,
} from "acp-client/node"

type AcpClientManagedInstallApi = {
  getInstalledAgent: typeof getInstalledAgent
  listInstalledAgents: typeof listInstalledAgents
  ensureAgentInstalled: typeof ensureAgentInstalled
  updateAgent: typeof updateAgent
  resolveInstalledAgentProcessSpec: typeof resolveInstalledAgentProcessSpec
}

type AgentInstallServiceOptions = {
  registryService: ACPRegistryService
  cacheDir?: string
  now?: () => number
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

/** Creates the daemon policy wrapper around acp-client managed install operations. */
export function createAgentInstallService(
  options: AgentInstallServiceOptions,
): DaemonAgentInstallService {
  const cacheDir = resolve(options.cacheDir ?? getDaemonAgentInstallCacheDir())
  const managedInstallApi = options.managedInstallApi ?? defaultManagedInstallApi

  function installOptions(extra: AgentInstallOptions = {}): AgentInstallOptions {
    return {
      cacheDir,
      now: options.now,
      ...extra,
    }
  }

  async function resolveAgent(input: DaemonManagedAgentInput): Promise<AgentDistribution> {
    if (typeof input.agent !== "string") {
      return input.agent
    }

    const configuredAgent = input.registry?.[input.agent]
    if (configuredAgent) {
      return configuredAgent
    }

    const registryEntry = await options.registryService.getAdapter(input.agent)
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
      return managedInstallApi.resolveInstalledAgentProcessSpec(
        agent,
        installOptions({
          installIfMissing: input.installIfMissing,
          maxInstalledAgeMs: MANAGED_AGENT_LAUNCH_FALLBACK_MAX_AGE_MS,
        }),
      )
    },
  }
}
