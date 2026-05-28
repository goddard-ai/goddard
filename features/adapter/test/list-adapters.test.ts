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
