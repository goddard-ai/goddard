import { mkdtemp } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join } from "node:path"
import type { ACPRegistryService } from "@goddard-ai/daemon-plugin"
import type { AgentDistribution } from "@goddard-ai/schema/agent-distribution"
import { expect, test } from "bun:test"

import { createAgentInstallService } from "../src/agent-install-service.ts"
import type { ManagedAgentUsageState } from "../src/managed-agent-usage.ts"

function createAgent(id: string): AgentDistribution {
  return {
    id,
    name: id,
    version: "1.0.0",
    description: `${id} agent`,
    distribution: {
      npx: {
        package: id,
        cmd: id,
      },
    },
  }
}

function createRegistryService(registry: Record<string, AgentDistribution>): ACPRegistryService {
  return {
    async listAdapters() {
      return {
        adapters: Object.values(registry),
        registrySource: "cache",
        lastSuccessfulSyncAt: "2026-06-08T00:00:00.000Z",
        stale: false,
        lastError: null,
      }
    },
    async getAdapter(id) {
      return {
        adapters: Object.values(registry),
        adapter: registry[id] ?? null,
        registrySource: "cache",
        lastSuccessfulSyncAt: "2026-06-08T00:00:00.000Z",
        stale: false,
        lastError: null,
      }
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

async function waitForCondition(condition: () => boolean) {
  for (let attempt = 0; attempt < 20; attempt += 1) {
    if (condition()) {
      return
    }

    await new Promise<void>((resolve) => setTimeout(resolve, 0))
  }
}

test("agent install service resolves configured and registry agents", async () => {
  const registryAgent = createAgent("registry-agent")
  const configuredAgent = createAgent("configured-agent")
  const service = createAgentInstallService({
    registryService: createRegistryService({ "registry-agent": registryAgent }),
    cacheDir: await mkdtemp(join(tmpdir(), "goddard-agent-install-service-")),
  })

  await expect(service.resolveAgent({ agent: "registry-agent" })).resolves.toEqual(registryAgent)
  await expect(
    service.resolveAgent({
      agent: "configured-agent",
      registry: {
        "configured-agent": configuredAgent,
      },
    }),
  ).resolves.toEqual(configuredAgent)
  await expect(service.resolveAgent({ agent: "missing-agent" })).rejects.toThrow(
    "Managed ACP agent not found: missing-agent",
  )
})

test("agent install service forwards deterministic cache options and launch fallback policy", async () => {
  const agent = createAgent("cache-agent")
  const cacheDir = await mkdtemp(join(tmpdir(), "goddard-agent-install-service-"))
  const calls: unknown[] = []
  const service = createAgentInstallService({
    registryService: createRegistryService({}),
    cacheDir,
    now: () => 1780848000000,
    managedInstallApi: {
      async getInstalledAgent(agentInput, options) {
        calls.push(["getInstalledAgent", agentInput, options])
        return { status: "installed", agent: createInstalledAgent(agent.id) }
      },
      async listInstalledAgents(options) {
        calls.push(["listInstalledAgents", options])
        return [createInstalledAgent(agent.id)]
      },
      async ensureAgentInstalled(agentInput, options) {
        calls.push(["ensureAgentInstalled", agentInput, options])
        return { agent: createInstalledAgent(agentInput.id), installed: true, updated: false }
      },
      async updateAgent(agentInput, options) {
        calls.push(["updateAgent", agentInput, options])
        return {
          agent: createInstalledAgent(agentInput.id),
          checkedAt: "2026-06-08T00:00:00.000Z",
          updated: false,
        }
      },
      async resolveInstalledAgentProcessSpec(agentInput, options) {
        calls.push(["resolveInstalledAgentProcessSpec", agentInput, options])
        return { cmd: agentInput.id, args: [], env: undefined }
      },
    },
  })

  await service.getInstalledAgent({ agent })
  await service.listInstalledAgents()
  await service.ensureAgentInstalled({ agent })
  await service.updateAgent({ agent })
  await service.resolveInstalledAgentProcessSpec({ agent, installIfMissing: true })

  expect(calls).toEqual([
    ["getInstalledAgent", agent, { cacheDir, now: expect.any(Function) }],
    ["listInstalledAgents", { cacheDir, now: expect.any(Function) }],
    ["ensureAgentInstalled", agent, { cacheDir, now: expect.any(Function) }],
    ["updateAgent", agent, { cacheDir, now: expect.any(Function) }],
    [
      "resolveInstalledAgentProcessSpec",
      agent,
      {
        cacheDir,
        now: expect.any(Function),
        installIfMissing: true,
        maxInstalledAgeMs: 90 * 24 * 60 * 60 * 1000,
      },
    ],
  ])
})

test("agent install service records usage after resolving a managed launch", async () => {
  const agent = createAgent("usage-agent")
  const usageStore = createUsageStore()
  const service = createAgentInstallService({
    registryService: createRegistryService({}),
    cacheDir: await mkdtemp(join(tmpdir(), "goddard-agent-install-service-")),
    now: () => Date.parse("2026-06-08T00:00:00.000Z"),
    usageStore: usageStore.store,
    managedInstallApi: {
      async getInstalledAgent() {
        return { status: "missing" }
      },
      async listInstalledAgents() {
        return []
      },
      async ensureAgentInstalled(agentInput) {
        return { agent: createInstalledAgent(agentInput.id), installed: true, updated: false }
      },
      async updateAgent(agentInput) {
        return {
          agent: createInstalledAgent(agentInput.id),
          checkedAt: "2026-06-08T00:00:00.000Z",
          updated: false,
        }
      },
      async resolveInstalledAgentProcessSpec(agentInput) {
        return { cmd: agentInput.id, args: [] }
      },
    },
  })

  await service.resolveInstalledAgentProcessSpec({ agent, installIfMissing: true })

  expect(usageStore.state).toEqual({
    "usage-agent": {
      lastUsedAt: "2026-06-08T00:00:00.000Z",
    },
  })
})

test("agent install service does not gate launch resolution behind background update work", async () => {
  const agent = createAgent("overlap-agent")
  const calls: string[] = []
  let releaseUpdate = () => {}
  const updateReleased = new Promise<void>((resolve) => {
    releaseUpdate = resolve
  })
  const service = createAgentInstallService({
    registryService: createRegistryService({}),
    cacheDir: await mkdtemp(join(tmpdir(), "goddard-agent-install-service-")),
    managedInstallApi: {
      async getInstalledAgent() {
        return { status: "missing" }
      },
      async listInstalledAgents() {
        return []
      },
      async ensureAgentInstalled(agentInput) {
        return { agent: createInstalledAgent(agentInput.id), installed: true, updated: false }
      },
      async updateAgent(agentInput) {
        calls.push("update:start")
        await updateReleased
        calls.push("update:end")
        return {
          agent: createInstalledAgent(agentInput.id),
          checkedAt: "2026-06-08T00:00:00.000Z",
          updated: false,
        }
      },
      async resolveInstalledAgentProcessSpec(agentInput) {
        calls.push("launch")
        return { cmd: agentInput.id, args: ["--ready"] }
      },
    },
  })

  const update = service.updateAgent({ agent })

  await waitForCondition(() => calls.length > 0)
  expect(calls).toEqual(["update:start"])

  const launch = service.resolveInstalledAgentProcessSpec({ agent, installIfMissing: true })
  await expect(launch).resolves.toEqual({
    cmd: agent.id,
    args: ["--ready"],
  })
  expect(calls).toEqual(["update:start", "launch"])

  releaseUpdate()

  await expect(update).resolves.toEqual({
    agent: createInstalledAgent(agent.id),
    checkedAt: "2026-06-08T00:00:00.000Z",
    updated: false,
  })
  expect(calls).toEqual(["update:start", "launch", "update:end"])
})
