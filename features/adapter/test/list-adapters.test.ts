import { mkdtemp, rm } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join } from "node:path"
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

      expect(installedResponse.adapters.map((adapter) => adapter.id)).toContain("registry-agent")
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
})
