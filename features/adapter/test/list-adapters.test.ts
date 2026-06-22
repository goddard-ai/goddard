import { mkdtemp, rm } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join } from "node:path"
import type { ManagedAgentInstallService } from "@goddard-ai/managed-agent/daemon/install-service"
import { createAcpRegistryService } from "acp-client/node"
import { describe, expect, test } from "bun:test"

import { installCatalogAdapter, listAdapters } from "../src/list-adapters.ts"

async function withIsolatedHome(callback: () => Promise<void>) {
  const previousHome = process.env.HOME
  const previousPath = process.env.PATH
  const homeDir = await mkdtemp(join(tmpdir(), "goddard-adapter-installations-"))

  process.env.HOME = homeDir
  process.env.PATH = ""
  try {
    await callback()
  } finally {
    if (previousHome === undefined) {
      delete process.env.HOME
    } else {
      process.env.HOME = previousHome
    }
    if (previousPath === undefined) {
      delete process.env.PATH
    } else {
      process.env.PATH = previousPath
    }
    await rm(homeDir, { recursive: true, force: true })
  }
}

function createAgentInstallService(
  statuses: Record<
    string,
    Awaited<ReturnType<ManagedAgentInstallService["getInstalledAgent"]>>
  > = {},
): ManagedAgentInstallService {
  return {
    cacheDir: "/tmp/acp-client",
    async resolveAgent({ agent }) {
      if (typeof agent !== "string") {
        return agent
      }

      return {
        id: agent,
        name: agent,
        version: "1.0.0",
        description: `${agent} agent`,
        distribution: { npx: { package: agent } },
      }
    },
    async getInstalledAgent({ agent }) {
      const agentId = typeof agent === "string" ? agent : agent.id
      return statuses[agentId] ?? { status: "missing" }
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
    async resolveInstalledAgentProcessSpec({ agent }) {
      return { cmd: typeof agent === "string" ? agent : agent.id, args: [] }
    },
  }
}

function createInstalledAgent(agentId: string) {
  return {
    agentId,
    version: "1.0.0",
    distributionHash: `${agentId}-hash`,
    method: "npx" as const,
    platform: undefined,
    installDir: `/tmp/${agentId}`,
    installedAt: "2026-06-08T00:00:00.000Z",
    updatedAt: "2026-06-08T00:00:00.000Z",
  }
}

describe("adapter listing", () => {
  test("merges config-declared adapters and resolves a valid default adapter", async () => {
    await expect(
      listAdapters(
        {
          registryService: {
            async listAdapters() {
              return {
                adapters: [
                  {
                    id: "registry-agent",
                    name: "Registry Agent",
                    version: "1.0.0",
                    description: "Registry-provided adapter",
                    distribution: { npx: { package: "registry-agent" } },
                    unofficial: false,
                    source: "registry" as const,
                  },
                ],
                registrySource: "cache",
                lastSuccessfulSyncAt: "2026-04-11T00:00:00.000Z",
                stale: false,
                lastError: null,
              }
            },
          },
          agentInstallService: createAgentInstallService(),
          configProvider: {
            async getRootConfig() {
              return {
                config: {
                  session: {
                    agent: "local-acp",
                  },
                  registry: {
                    "local-acp": {
                      id: "local-acp",
                      name: "Local ACP",
                      version: "1.0.0",
                      description: "Config-provided adapter",
                      distribution: { npx: { package: "local-acp" } },
                    },
                  },
                },
              }
            },
          },
        },
        { cwd: "/repo", includeUninstalled: true },
      ),
    ).resolves.toMatchObject({
      defaultAdapterId: "local-acp",
      registrySource: "cache",
      adapters: [
        {
          id: "local-acp",
          source: "config",
        },
        {
          id: "registry-agent",
          source: "registry",
        },
      ],
    })
  })

  test("lists adapters from acp-client registry data plus project config entries", async () => {
    const cacheDir = await mkdtemp(join(tmpdir(), "goddard-acp-client-registry-"))
    try {
      const response = await listAdapters(
        {
          registryService: createAcpRegistryService({
            cacheDir,
            registryUrl: join(tmpdir(), "missing-acp-registry"),
            registry: {
              "service-acp": {
                id: "service-acp",
                name: "Service ACP",
                version: "1.0.0",
                description: "Registry service adapter",
                distribution: { npx: { package: "service-acp" } },
              },
            },
          }),
          agentInstallService: createAgentInstallService(),
          configProvider: {
            async getRootConfig() {
              return {
                config: {
                  session: {
                    agent: "project-acp",
                  },
                  registry: {
                    "project-acp": {
                      id: "project-acp",
                      name: "Project ACP",
                      version: "1.0.0",
                      description: "Project config adapter",
                      distribution: { npx: { package: "project-acp" } },
                    },
                  },
                },
              }
            },
          },
        },
        { cwd: "/repo", includeUninstalled: true },
      )

      expect(response.registrySource).toBe("fallback")
      expect(response.defaultAdapterId).toBe("project-acp")
      expect(response.adapters).toContainEqual(
        expect.objectContaining({
          id: "service-acp",
          source: "config",
        }),
      )
      expect(response.adapters).toContainEqual(
        expect.objectContaining({
          id: "project-acp",
          source: "config",
        }),
      )
      expect(response.adapters.length).toBeGreaterThan(2)
    } finally {
      await rm(cacheDir, { recursive: true, force: true })
    }
  })

  test("uses agents.default as the adapter default when no narrower default is configured", async () => {
    await expect(
      listAdapters(
        {
          registryService: {
            async listAdapters() {
              return {
                adapters: [],
                registrySource: "cache",
                lastSuccessfulSyncAt: "2026-04-11T00:00:00.000Z",
                stale: false,
                lastError: null,
              }
            },
          },
          agentInstallService: createAgentInstallService(),
          configProvider: {
            async getRootConfig() {
              return {
                config: {
                  agents: {
                    default: "local-acp",
                  },
                  registry: {
                    "local-acp": {
                      id: "local-acp",
                      name: "Local ACP",
                      version: "1.0.0",
                      description: "Config-provided adapter",
                      distribution: { npx: { package: "local-acp" } },
                    },
                  },
                },
              }
            },
          },
        },
        { cwd: "/repo" },
      ),
    ).resolves.toMatchObject({
      defaultAdapterId: "local-acp",
    })
  })

  test("omits uninstalled registry adapters from launch listings", async () => {
    await withIsolatedHome(async () => {
      const context = {
        registryService: {
          async listAdapters() {
            return {
              adapters: [
                {
                  id: "aaa-agent",
                  name: "AAA Agent",
                  version: "1.0.0",
                  description: "Alphabetically early registry adapter",
                  distribution: { npx: { package: "aaa-agent" } },
                  unofficial: false,
                  source: "registry" as const,
                },
                {
                  id: "registry-agent",
                  name: "Registry Agent",
                  version: "1.0.0",
                  description: "Registry-provided adapter",
                  distribution: { npx: { package: "registry-agent" } },
                  unofficial: false,
                  source: "registry" as const,
                },
              ],
              registrySource: "cache" as const,
              lastSuccessfulSyncAt: "2026-04-11T00:00:00.000Z",
              stale: false,
              lastError: null,
            }
          },
        },
        agentInstallService: createAgentInstallService(),
        configProvider: {
          async getRootConfig() {
            return {
              config: {
                registry: {
                  "local-acp": {
                    id: "local-acp",
                    name: "Local ACP",
                    version: "1.0.0",
                    description: "Config-provided adapter",
                    distribution: { npx: { package: "local-acp" } },
                  },
                },
              },
            }
          },
        },
      }

      await expect(listAdapters(context, { cwd: "/repo" })).resolves.toMatchObject({
        defaultAdapterId: null,
        adapters: [
          {
            id: "local-acp",
            source: "config",
          },
        ],
        installations: expect.arrayContaining([
          {
            adapterId: "aaa-agent",
            installable: true,
            installed: false,
            method: "npx",
          },
          {
            adapterId: "registry-agent",
            installable: true,
            installed: false,
            method: "npx",
          },
          {
            adapterId: "local-acp",
            installable: false,
            installed: true,
            method: "config",
          },
        ]),
      })

      await installCatalogAdapter(context, { adapterId: "registry-agent" })

      const installedResponse = await listAdapters(context, { cwd: "/repo" })
      const settingsResponse = await listAdapters(context, {
        cwd: "/repo",
        includeUninstalled: true,
      })

      expect(installedResponse.adapters.map((adapter) => adapter.id)).toContain("registry-agent")
      expect(settingsResponse.adapters.map((adapter) => adapter.id)).toEqual([
        "local-acp",
        "registry-agent",
        "aaa-agent",
      ])
    })
  })

  test("omits default adapter when the configured agent is not in the merged catalog", async () => {
    await expect(
      listAdapters(
        {
          registryService: {
            async listAdapters() {
              return {
                adapters: [],
                registrySource: "fallback",
                lastSuccessfulSyncAt: null,
                stale: false,
                lastError: null,
              }
            },
          },
          agentInstallService: createAgentInstallService(),
          configProvider: {
            async getRootConfig() {
              return {
                config: {
                  session: {
                    agent: "missing-acp",
                  },
                },
              }
            },
          },
        },
        { cwd: "/repo" },
      ),
    ).resolves.toMatchObject({
      defaultAdapterId: null,
      adapters: [],
    })
  })

  test("surfaces managed install status without local install paths", async () => {
    await expect(
      listAdapters(
        {
          registryService: {
            async listAdapters() {
              return {
                adapters: [
                  {
                    id: "managed-acp",
                    name: "Managed ACP",
                    version: "1.0.0",
                    description: "Managed adapter",
                    distribution: { npx: { package: "managed-acp" } },
                    unofficial: false,
                    source: "registry" as const,
                  },
                ],
                registrySource: "cache",
                lastSuccessfulSyncAt: "2026-04-11T00:00:00.000Z",
                stale: false,
                lastError: null,
              }
            },
          },
          agentInstallService: createAgentInstallService({
            "managed-acp": {
              status: "installed",
              agent: createInstalledAgent("managed-acp"),
            },
          }),
          configProvider: {
            async getRootConfig() {
              return {
                config: {
                  agents: {
                    managed: {
                      "managed-acp": {
                        install: "beforeUse",
                        update: "daily",
                      },
                    },
                  },
                },
              }
            },
          },
        },
        { cwd: "/repo" },
      ),
    ).resolves.toMatchObject({
      adapters: [
        {
          id: "managed-acp",
          managedInstall: {
            managed: true,
            install: "beforeUse",
            update: "daily",
            state: {
              status: "installed",
              agent: {
                agentId: "managed-acp",
                version: "1.0.0",
                method: "npx",
                installedAt: "2026-06-08T00:00:00.000Z",
                updatedAt: "2026-06-08T00:00:00.000Z",
              },
            },
          },
        },
      ],
    })
  })

  test("lists managed registry adapters while hiding ordinary uninstalled registry adapters", async () => {
    await withIsolatedHome(async () => {
      const response = await listAdapters(
        {
          registryService: {
            async listAdapters() {
              return {
                adapters: [
                  {
                    id: "managed-acp",
                    name: "Managed ACP",
                    version: "1.0.0",
                    description: "Managed adapter",
                    distribution: { npx: { package: "managed-acp" } },
                    unofficial: false,
                    source: "registry" as const,
                  },
                  {
                    id: "ordinary-acp",
                    name: "Ordinary ACP",
                    version: "1.0.0",
                    description: "Ordinary adapter",
                    distribution: { npx: { package: "ordinary-acp" } },
                    unofficial: false,
                    source: "registry" as const,
                  },
                ],
                registrySource: "cache",
                lastSuccessfulSyncAt: "2026-04-11T00:00:00.000Z",
                stale: false,
                lastError: null,
              }
            },
          },
          agentInstallService: createAgentInstallService(),
          configProvider: {
            async getRootConfig() {
              return {
                config: {
                  agents: {
                    managed: {
                      "managed-acp": {
                        install: "beforeUse",
                      },
                    },
                  },
                },
              }
            },
          },
        },
        { cwd: "/repo" },
      )

      expect(response.adapters.map((adapter) => adapter.id)).toEqual(["managed-acp"])
      expect(response.installations).toEqual(
        expect.arrayContaining([
          {
            adapterId: "managed-acp",
            installable: true,
            installed: false,
            method: "npx",
          },
          {
            adapterId: "ordinary-acp",
            installable: true,
            installed: false,
            method: "npx",
          },
        ]),
      )
    })
  })

  test("surfaces failed managed install status with sanitized previous install metadata", async () => {
    const response = await listAdapters(
      {
        registryService: {
          async listAdapters() {
            return {
              adapters: [
                {
                  id: "failed-acp",
                  name: "Failed ACP",
                  version: "1.0.0",
                  description: "Failed adapter",
                  distribution: { npx: { package: "failed-acp" } },
                  unofficial: false,
                  source: "registry" as const,
                },
              ],
              registrySource: "cache",
              lastSuccessfulSyncAt: "2026-04-11T00:00:00.000Z",
              stale: false,
              lastError: null,
            }
          },
        },
        agentInstallService: createAgentInstallService({
          "failed-acp": {
            status: "failed",
            lastError: "update failed",
            checkedAt: "2026-06-08T00:00:00.000Z",
            agent: createInstalledAgent("failed-acp"),
          },
        }),
        configProvider: {
          async getRootConfig() {
            return {
              config: {
                agents: {
                  managed: {
                    "failed-acp": {
                      update: "daily",
                    },
                  },
                },
              },
            }
          },
        },
      },
      { cwd: "/repo" },
    )

    expect(response.adapters[0]?.managedInstall).toEqual({
      managed: true,
      update: "daily",
      state: {
        status: "failed",
        lastError: "update failed",
        checkedAt: "2026-06-08T00:00:00.000Z",
        agent: {
          agentId: "failed-acp",
          version: "1.0.0",
          method: "npx",
          installedAt: "2026-06-08T00:00:00.000Z",
          updatedAt: "2026-06-08T00:00:00.000Z",
        },
      },
    })
    expect(JSON.stringify(response)).not.toContain("installDir")
    expect(JSON.stringify(response)).not.toContain("distributionHash")
  })
})
