import { mkdtemp, rm, stat } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join } from "node:path"
import type { ManagedAgentInstallService } from "@goddard-ai/agent/daemon/install-service"
import {
  resolveManagedAgentLaunchProcessSpec as resolveLaunchAgentProcessSpec,
  resolveUnmanagedAgentProcessSpec,
} from "@goddard-ai/agent/daemon/launch-process"
import { agentBinaryPlatforms, type AgentDistribution } from "@goddard-ai/schema/agent-distribution"
import * as acp from "acp-client/protocol"
import { afterEach, expect, test, vi } from "bun:test"

import { injectSystemPrompt } from "../src/daemon/manager.ts"

const cleanupDirs: string[] = []
const originalHome = process.env.HOME
const originalFetch = globalThis.fetch

afterEach(async () => {
  globalThis.fetch = originalFetch

  if (originalHome === undefined) {
    delete process.env.HOME
  } else {
    process.env.HOME = originalHome
  }

  while (cleanupDirs.length > 0) {
    await rm(cleanupDirs.pop()!, { recursive: true, force: true, maxRetries: 5, retryDelay: 100 })
  }
})

function createAgent(id: string): AgentDistribution {
  return {
    id,
    name: id,
    version: "1.0.0",
    description: `${id} agent`,
    distribution: {
      npx: {
        package: id,
      },
    },
  }
}

function createManagedAgentService(
  registry: Record<string, AgentDistribution>,
  resolveInstalledAgentProcessSpec: ManagedAgentInstallService["resolveInstalledAgentProcessSpec"] = async ({
    agent,
  }) => ({
    cmd: typeof agent === "string" ? agent : agent.id,
    args: [],
  }),
): ManagedAgentInstallService {
  return {
    cacheDir: "/tmp/acp-client",
    async resolveAgent({ agent, registry: configuredRegistry }) {
      if (typeof agent !== "string") {
        return agent
      }

      const resolvedAgent = configuredRegistry?.[agent] ?? registry[agent]
      if (!resolvedAgent) {
        throw new Error(`ACP agent not found: ${agent}`)
      }

      return resolvedAgent
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
    async updateAgent({ agent }) {
      const agentId = typeof agent === "string" ? agent : agent.id
      return {
        agent: createInstalledAgent(agentId),
        checkedAt: "2026-06-08T00:00:00.000Z",
        updated: false,
      }
    },
    resolveInstalledAgentProcessSpec,
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

test("resolveUnmanagedAgentProcessSpec installs archive-backed unmanaged binaries", async () => {
  const homeDir = await mkdtemp(join(tmpdir(), "goddard-home-"))
  cleanupDirs.push(homeDir)
  process.env.HOME = homeDir

  const fetchMock = vi.fn(async () => new Response("#!/bin/sh\nexit 0\n", { status: 200 }))
  globalThis.fetch = fetchMock as unknown as typeof fetch

  const binaryTarget = {
    archive:
      "https://raw.githubusercontent.com/agentclientprotocol/registry/refs/heads/main/codex-acp/agent",
    cmd: "bin/agent",
    args: ["--serve"],
    env: {
      FOO: "bar",
    },
  }

  const agent = {
    id: "node-agent",
    name: "Node Agent",
    version: "1.0.0",
    description: "Archive-backed ACP test agent.",
    distribution: {
      binary: Object.fromEntries(
        agentBinaryPlatforms.map((platform) => [platform, binaryTarget]),
      ) as Record<(typeof agentBinaryPlatforms)[number], typeof binaryTarget>,
    },
  }

  const firstSpec = await resolveUnmanagedAgentProcessSpec(agent)
  const secondSpec = await resolveUnmanagedAgentProcessSpec(agent)

  expect(fetchMock).toHaveBeenCalledTimes(1)
  expect(firstSpec).toEqual(secondSpec)
  expect(firstSpec.args).toEqual(["--serve"])
  expect(firstSpec.env).toEqual({ FOO: "bar" })
  expect(firstSpec.cmd.startsWith(join(homeDir, ".goddard", "binaries"))).toBe(true)
  expect(firstSpec.cmd.endsWith(join("bin", "agent"))).toBe(true)
  await expect(stat(firstSpec.cmd)).resolves.toBeTruthy()
})

test("resolveLaunchAgentProcessSpec uses managed installs before launching configured agents", async () => {
  const calls: unknown[] = []
  const managedAgent = createManagedAgentService({}, async (input) => {
    calls.push(input)
    return {
      cmd: "/tmp/managed-agent/bin/agent",
      args: ["serve"],
      env: { MANAGED_AGENT: "1" },
    }
  })

  await expect(
    resolveLaunchAgentProcessSpec(managedAgent, {
      agent: "managed-agent",
      managedAgents: {
        "managed-agent": {
          install: "beforeUse",
        },
      },
    }),
  ).resolves.toEqual({
    cmd: "/tmp/managed-agent/bin/agent",
    args: ["serve"],
    env: { MANAGED_AGENT: "1" },
  })

  expect(calls).toEqual([
    {
      agent: "managed-agent",
      registry: undefined,
      installIfMissing: true,
    },
  ])
})

test("resolveLaunchAgentProcessSpec forwards configured registry overrides to managed installs", async () => {
  const configuredAgent = createAgent("configured-agent")
  const calls: unknown[] = []
  const managedAgent = createManagedAgentService({}, async (input) => {
    calls.push(input)
    return {
      cmd: "/tmp/configured-agent/bin/agent",
      args: [],
    }
  })

  await resolveLaunchAgentProcessSpec(managedAgent, {
    agent: "configured-agent",
    registry: {
      "configured-agent": configuredAgent,
    },
    managedAgents: {
      "configured-agent": {
        install: "beforeUse",
      },
    },
  })

  expect(calls).toEqual([
    {
      agent: "configured-agent",
      registry: {
        "configured-agent": configuredAgent,
      },
      installIfMissing: true,
    },
  ])
})

test("resolveLaunchAgentProcessSpec preserves unmanaged launch resolution", async () => {
  const managedAgent = createManagedAgentService(
    {
      "unmanaged-agent": createAgent("unmanaged-agent"),
    },
    async () => {
      throw new Error("managed install should not be used")
    },
  )

  await expect(
    resolveLaunchAgentProcessSpec(managedAgent, {
      agent: "unmanaged-agent",
      managedAgents: {
        "other-agent": {
          install: "beforeUse",
        },
      },
    }),
  ).resolves.toEqual({
    cmd: "npx",
    args: ["-y", "unmanaged-agent"],
    env: undefined,
  })
})

test("resolveLaunchAgentProcessSpec propagates managed install failures", async () => {
  const managedAgent = createManagedAgentService({}, async () => {
    throw new Error("managed install failed")
  })

  await expect(
    resolveLaunchAgentProcessSpec(managedAgent, {
      agent: "managed-agent",
      managedAgents: {
        "managed-agent": {
          install: "beforeUse",
        },
      },
    }),
  ).rejects.toThrow("managed install failed")
})

test("injectSystemPrompt leaves prompts unchanged when the daemon system prompt is empty", () => {
  const request = {
    sessionId: "acp-session-1",
    prompt: [{ type: "text", text: "Say hello." }],
  } satisfies acp.PromptRequest

  expect(injectSystemPrompt(request, "")).toEqual(request)
})

test("injectSystemPrompt prepends the daemon system prompt with the goddard tag name", () => {
  const request = {
    sessionId: "acp-session-1",
    prompt: [{ type: "text", text: "Say hello." }],
  } satisfies acp.PromptRequest

  expect(injectSystemPrompt(request, "Keep responses short.")).toEqual({
    sessionId: "acp-session-1",
    prompt: [
      {
        type: "text",
        text: '<system-prompt name="goddard">Keep responses short.</system-prompt>',
      },
      { type: "text", text: "Say hello." },
    ],
  } satisfies acp.PromptRequest)
})
