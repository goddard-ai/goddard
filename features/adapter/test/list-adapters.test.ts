import { mkdtemp, rm } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { createAcpRegistryService } from "acp-client/node"
import { describe, expect, test } from "bun:test"

import { listAdapters } from "../src/list-adapters.ts"

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
          configManager: {
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
        { cwd: "/repo" },
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
          configManager: {
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
        { cwd: "/repo" },
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
          configManager: {
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
