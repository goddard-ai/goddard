import { createHash } from "node:crypto"
import type {
  DaemonAgentInstallService,
  DaemonConfigProvider,
  DaemonLogger,
} from "@goddard-ai/daemon-plugin"
import type { AgentDistribution } from "@goddard-ai/schema/agent-distribution"
import type { AgentsConfig } from "@goddard-ai/schema/config"
import { getErrorMessage } from "radashi"

type ManagedAgentUpdateRootConfig = {
  agents?: AgentsConfig
  registry?: Record<string, AgentDistribution>
}

export type ManagedAgentUpdateCheckState = Record<
  string,
  {
    readonly checkedAt: string
    readonly configFingerprint: string
  }
>

type ManagedAgentUpdateCheckStore = {
  readonly get: () => ManagedAgentUpdateCheckState | undefined
  readonly set: (state: ManagedAgentUpdateCheckState) => void
}

type ManagedAgentUpdateTimer = unknown
type ManagedAgentUpdateSetTimeout = (
  callback: () => void,
  delayMs: number,
) => ManagedAgentUpdateTimer
type ManagedAgentUpdateClearTimeout = (timer: ManagedAgentUpdateTimer) => void

type ManagedAgentUpdateSchedulerOptions = {
  readonly configProvider: DaemonConfigProvider<ManagedAgentUpdateRootConfig>
  readonly agentInstallService: DaemonAgentInstallService
  readonly updateCheckStore: ManagedAgentUpdateCheckStore
  readonly logger: DaemonLogger
  readonly now?: () => number
  readonly setTimeout?: ManagedAgentUpdateSetTimeout
  readonly clearTimeout?: ManagedAgentUpdateClearTimeout
}

const DAILY_UPDATE_INTERVAL_MS = 24 * 60 * 60 * 1000

/** Runs one non-blocking daily update scheduler for globally managed ACP agents. */
export function createManagedAgentUpdateScheduler(options: ManagedAgentUpdateSchedulerOptions) {
  const setTimer: ManagedAgentUpdateSetTimeout =
    options.setTimeout ?? ((callback, delayMs) => setTimeout(callback, delayMs))
  const clearTimer: ManagedAgentUpdateClearTimeout =
    options.clearTimeout ?? ((timer) => clearTimeout(timer as ReturnType<typeof setTimeout>))
  let timer: ManagedAgentUpdateTimer | null = null
  let closed = false

  function schedule(delayMs: number) {
    if (closed) {
      return
    }

    timer = setTimer(() => {
      timer = null
      void runManagedAgentUpdateChecks(options).finally(() => {
        schedule(DAILY_UPDATE_INTERVAL_MS)
      })
    }, delayMs)
  }

  return {
    start() {
      schedule(0)
    },
    close() {
      closed = true
      if (timer) {
        clearTimer(timer)
        timer = null
      }
    },
  }
}

/** Runs due update checks once for managed ACP agents declared in global root config. */
export async function runManagedAgentUpdateChecks(options: ManagedAgentUpdateSchedulerOptions) {
  const nowMs = options.now?.() ?? Date.now()
  const checkedAt = new Date(nowMs).toISOString()
  const rootConfig = await options.configProvider.getRootConfig()
  const managedAgents = rootConfig.config.agents?.managed ?? {}
  const registry = rootConfig.config.registry
  const previousState = options.updateCheckStore.get() ?? {}
  const nextState: ManagedAgentUpdateCheckState = { ...previousState }

  for (const [agentId, managedAgent] of Object.entries(managedAgents).sort(([left], [right]) =>
    left.localeCompare(right),
  )) {
    if (managedAgent.update !== "daily") {
      continue
    }

    const configFingerprint = createConfigFingerprint({
      agentId,
      managedAgent,
      registryEntry: registry?.[agentId],
    })
    const previousCheck = previousState[agentId]
    if (previousCheck?.configFingerprint === configFingerprint) {
      const previousCheckedAtMs = Date.parse(previousCheck.checkedAt)
      if (
        Number.isFinite(previousCheckedAtMs) &&
        nowMs - previousCheckedAtMs < DAILY_UPDATE_INTERVAL_MS
      ) {
        continue
      }
    }

    try {
      await options.agentInstallService.updateAgent({
        agent: agentId,
        registry,
      })
      nextState[agentId] = {
        checkedAt,
        configFingerprint,
      }
      options.updateCheckStore.set(nextState)
    } catch (error) {
      nextState[agentId] = {
        checkedAt,
        configFingerprint,
      }
      options.updateCheckStore.set(nextState)
      options.logger.log("agent_update.failed", {
        agentId,
        errorMessage: getErrorMessage(error),
      })
    }
  }
}

function createConfigFingerprint(value: unknown) {
  return createHash("sha256").update(stableStringify(value)).digest("hex")
}

function stableStringify(value: unknown): string {
  if (Array.isArray(value)) {
    return `[${value.map((entry) => stableStringify(entry)).join(",")}]`
  }

  if (value && typeof value === "object") {
    return `{${Object.entries(value)
      .sort(([left], [right]) => left.localeCompare(right))
      .map(([key, entry]) => `${JSON.stringify(key)}:${stableStringify(entry)}`)
      .join(",")}}`
  }

  return JSON.stringify(value)
}
