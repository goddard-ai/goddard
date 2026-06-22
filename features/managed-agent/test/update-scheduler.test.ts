import type { DaemonConfigProvider, DaemonLogger } from "@goddard-ai/daemon-plugin"
import type { ManagedAgentService } from "@goddard-ai/managed-agent/daemon"
import type { AgentDistribution } from "@goddard-ai/schema/agent-distribution"
import { expect, test } from "bun:test"

import {
  createManagedAgentUpdateScheduler,
  runManagedAgentUpdateChecks,
  type ManagedAgentUpdateCheckState,
} from "../src/daemon/update-scheduler.ts"
import type { ManagedAgentUsageState } from "../src/daemon/usage-store.ts"

type TestRootConfig = Awaited<ReturnType<DaemonConfigProvider["getRootConfig"]>>["config"]

function createAgent(id: string, version = "1.0.0"): AgentDistribution {
  return {
    id,
    name: id,
    version,
    description: `${id} agent`,
    distribution: {
      npx: {
        package: id,
      },
    },
  }
}

function createConfigProvider(config: TestRootConfig): DaemonConfigProvider {
  const snapshot = {
    globalRoot: "/tmp/goddard",
    localRoot: "/tmp/repo",
    config,
    version: 1,
    loadedAt: "2026-06-08T00:00:00.000Z",
  }

  return {
    async getRootConfig() {
      return snapshot
    },
    getLastKnownRootConfig() {
      return snapshot
    },
  }
}

function createAgentInstallService(
  updateAgent: ManagedAgentService["updateAgent"],
): ManagedAgentService {
  return {
    cacheDir: "/tmp/acp-client",
    async resolveAgent({ agent }) {
      return typeof agent === "string" ? createAgent(agent) : agent
    },
    async getInstalledAgent() {
      return { status: "missing" }
    },
    async listInstalledAgents() {
      return []
    },
    async ensureAgentInstalled({ agent }) {
      const agentId = typeof agent === "string" ? agent : agent.id
      return {
        agent: createInstalledAgent(agentId),
        installed: true,
        updated: false,
      }
    },
    updateAgent,
    async resolveInstalledAgentProcessSpec({ agent }) {
      const agentId = typeof agent === "string" ? agent : agent.id
      return { cmd: agentId, args: [] }
    },
  }
}

function createInstalledAgent(agentId: string) {
  return {
    agentId,
    version: "1.0.0",
    distributionHash: `${agentId}-hash`,
    method: "npx" as const,
    installDir: `/tmp/${agentId}`,
    installedAt: "2026-06-08T00:00:00.000Z",
    updatedAt: "2026-06-08T00:00:00.000Z",
  }
}

function createStateStore(initialState: ManagedAgentUpdateCheckState = {}) {
  let state = initialState

  return {
    get state() {
      return state
    },
    store: {
      get: () => state,
      set: (nextState: ManagedAgentUpdateCheckState) => {
        state = { ...nextState }
      },
    },
  }
}

function createUsageStore(initialState: ManagedAgentUsageState = {}) {
  let state = initialState

  return {
    get state() {
      return state
    },
    store: {
      get: () => state,
      set: (nextState: ManagedAgentUsageState) => {
        state = { ...nextState }
      },
    },
  }
}

function createLogger() {
  const events: Array<[string, Record<string, unknown> | undefined]> = []
  const logger: DaemonLogger = {
    log(event, fields) {
      events.push([event, fields])
    },
    snapshot() {
      return logger
    },
  }

  return { logger, events }
}

test("managed agent update checks update daily managed agents", async () => {
  const updatedAgentIds: string[] = []
  const stateStore = createStateStore()
  const usageStore = createUsageStore({
    "managed-agent": {
      lastUsedAt: "2026-06-07T00:00:00.000Z",
    },
  })

  await runManagedAgentUpdateChecks({
    configProvider: createConfigProvider({
      agents: {
        managed: {
          "managed-agent": {
            update: "daily",
          },
          "install-only-agent": {
            install: "beforeUse",
          },
        },
      },
    }),
    agentInstallService: createAgentInstallService(async ({ agent }) => {
      const agentId = typeof agent === "string" ? agent : agent.id
      updatedAgentIds.push(agentId)
      return {
        agent: createInstalledAgent(agentId),
        checkedAt: "2026-06-08T00:00:00.000Z",
        updated: false,
      }
    }),
    updateCheckStore: stateStore.store,
    usageStore: usageStore.store,
    logger: createLogger().logger,
    now: () => Date.parse("2026-06-08T00:00:00.000Z"),
  })

  expect(updatedAgentIds).toEqual(["managed-agent"])
  expect(stateStore.state["managed-agent"]?.checkedAt).toBe("2026-06-08T00:00:00.000Z")
  expect(stateStore.state["install-only-agent"]).toBeUndefined()
})

test("managed agent update checks skip fresh state until config changes", async () => {
  const updatedAgentIds: string[] = []
  const stateStore = createStateStore()
  const usageStore = createUsageStore({
    "managed-agent": {
      lastUsedAt: "2026-06-07T00:00:00.000Z",
    },
  })
  const agentInstallService = createAgentInstallService(async ({ agent }) => {
    const agentId = typeof agent === "string" ? agent : agent.id
    updatedAgentIds.push(agentId)
    return {
      agent: createInstalledAgent(agentId),
      checkedAt: "2026-06-08T00:00:00.000Z",
      updated: false,
    }
  })
  const baseInput = {
    agentInstallService,
    updateCheckStore: stateStore.store,
    usageStore: usageStore.store,
    logger: createLogger().logger,
  }

  await runManagedAgentUpdateChecks({
    ...baseInput,
    configProvider: createConfigProvider({
      agents: {
        managed: {
          "managed-agent": {
            update: "daily",
          },
        },
      },
      registry: {
        "managed-agent": createAgent("managed-agent", "1.0.0"),
      },
    }),
    now: () => Date.parse("2026-06-08T00:00:00.000Z"),
  })
  await runManagedAgentUpdateChecks({
    ...baseInput,
    configProvider: createConfigProvider({
      agents: {
        managed: {
          "managed-agent": {
            update: "daily",
          },
        },
      },
      registry: {
        "managed-agent": createAgent("managed-agent", "1.0.0"),
      },
    }),
    now: () => Date.parse("2026-06-08T12:00:00.000Z"),
  })
  await runManagedAgentUpdateChecks({
    ...baseInput,
    configProvider: createConfigProvider({
      agents: {
        managed: {
          "managed-agent": {
            update: "daily",
          },
        },
      },
      registry: {
        "managed-agent": createAgent("managed-agent", "2.0.0"),
      },
    }),
    now: () => Date.parse("2026-06-08T13:00:00.000Z"),
  })

  expect(updatedAgentIds).toEqual(["managed-agent", "managed-agent"])
  expect(stateStore.state["managed-agent"]?.checkedAt).toBe("2026-06-08T13:00:00.000Z")
})

test("managed agent update checks skip agents not used recently", async () => {
  const updatedAgentIds: string[] = []
  const stateStore = createStateStore()
  const usageStore = createUsageStore({
    "recent-agent": {
      lastUsedAt: "2026-05-10T00:00:00.000Z",
    },
    "stale-agent": {
      lastUsedAt: "2026-04-08T00:00:00.000Z",
    },
  })

  await runManagedAgentUpdateChecks({
    configProvider: createConfigProvider({
      agents: {
        managed: {
          "never-used-agent": {
            update: "daily",
          },
          "recent-agent": {
            update: "daily",
          },
          "stale-agent": {
            update: "daily",
          },
        },
      },
    }),
    agentInstallService: createAgentInstallService(async ({ agent }) => {
      const agentId = typeof agent === "string" ? agent : agent.id
      updatedAgentIds.push(agentId)
      return {
        agent: createInstalledAgent(agentId),
        checkedAt: "2026-06-08T00:00:00.000Z",
        updated: false,
      }
    }),
    updateCheckStore: stateStore.store,
    usageStore: usageStore.store,
    logger: createLogger().logger,
    now: () => Date.parse("2026-06-08T00:00:00.000Z"),
  })

  expect(updatedAgentIds).toEqual(["recent-agent"])
  expect(stateStore.state["recent-agent"]?.checkedAt).toBe("2026-06-08T00:00:00.000Z")
  expect(stateStore.state["never-used-agent"]).toBeUndefined()
  expect(stateStore.state["stale-agent"]).toBeUndefined()
})

test("managed agent update checks record and log failed updates", async () => {
  const stateStore = createStateStore()
  const usageStore = createUsageStore({
    "managed-agent": {
      lastUsedAt: "2026-06-07T00:00:00.000Z",
    },
  })
  const { logger, events } = createLogger()

  await runManagedAgentUpdateChecks({
    configProvider: createConfigProvider({
      agents: {
        managed: {
          "managed-agent": {
            update: "daily",
          },
        },
      },
    }),
    agentInstallService: createAgentInstallService(async () => {
      throw new Error("update failed")
    }),
    updateCheckStore: stateStore.store,
    usageStore: usageStore.store,
    logger,
    now: () => Date.parse("2026-06-08T00:00:00.000Z"),
  })

  expect(stateStore.state["managed-agent"]?.checkedAt).toBe("2026-06-08T00:00:00.000Z")
  expect(events).toEqual([
    [
      "agent_update.failed",
      {
        agentId: "managed-agent",
        errorMessage: "update failed",
      },
    ],
  ])
})

test("managed agent update scheduler clears pending timers on close", () => {
  let nextTimerId = 0
  const timers = new Set<number>()
  const clearedTimers = new Set<number>()
  const scheduler = createManagedAgentUpdateScheduler({
    configProvider: createConfigProvider({}),
    agentInstallService: createAgentInstallService(async ({ agent }) => ({
      agent: createInstalledAgent(typeof agent === "string" ? agent : agent.id),
      checkedAt: "2026-06-08T00:00:00.000Z",
      updated: false,
    })),
    updateCheckStore: createStateStore().store,
    usageStore: createUsageStore().store,
    logger: createLogger().logger,
    setTimeout: () => {
      const timer = (nextTimerId += 1)
      timers.add(timer)
      return timer
    },
    clearTimeout: (timer) => {
      clearedTimers.add(timer as number)
    },
  })

  scheduler.start()
  scheduler.close()

  expect(timers.size).toBe(1)
  expect(clearedTimers.size).toBe(1)
})
