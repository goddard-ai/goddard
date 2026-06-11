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
  const agentTaskTails = new Map<string, Promise<void>>()

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

  function runAgentTask<TResult>(agentId: string, task: () => Promise<TResult>) {
    // acp-client install/update/launch operations share per-agent cache state; queue them so
    // overlapping callers cannot observe another operation's result shape or partial writes.
    const previousTask = agentTaskTails.get(agentId) ?? Promise.resolve()
    const nextTask = previousTask.catch(() => {}).then(task)
    const nextTail = nextTask.then(
      () => {},
      () => {},
    )
    agentTaskTails.set(agentId, nextTail)
    void nextTail.finally(() => {
      if (agentTaskTails.get(agentId) === nextTail) {
        agentTaskTails.delete(agentId)
      }
    })

    return nextTask
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
      return runAgentTask(agent.id, () =>
        managedInstallApi.ensureAgentInstalled(agent, installOptions()),
      )
    },

    async updateAgent(input) {
      const agent = await resolveAgent(input)
      return runAgentTask(agent.id, () => managedInstallApi.updateAgent(agent, installOptions()))
    },

    async resolveInstalledAgentProcessSpec(input) {
      const agent = await resolveAgent(input)
      return runAgentTask(agent.id, () =>
        managedInstallApi.resolveInstalledAgentProcessSpec(
          agent,
          installOptions({
            installIfMissing: input.installIfMissing,
          }),
        ),
      )
    },
  }
}
