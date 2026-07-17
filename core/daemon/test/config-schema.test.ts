import { expect, test } from "bun:test"

import { buildRootConfigSchema, mergeRootConfigLayers } from "../src/config-schema.ts"
import {
  buildEditableRootConfigJsonSchema,
  buildGeneratedSchemaArtifacts,
} from "../src/json-schemas.ts"

test("daemon root config schema accepts session title generator model config", () => {
  const config = buildRootConfigSchema().parse({
    sessionTitles: {
      generator: {
        provider: "openai",
        model: "gpt-4.1-mini",
      },
    },
  }) as { sessionTitles?: { generator?: unknown } }

  expect(config.sessionTitles?.generator).toEqual({
    provider: "openai",
    model: "gpt-4.1-mini",
  })
})

test("daemon root config schema accepts worktree branch prefix config", () => {
  const config = buildRootConfigSchema().parse({
    worktrees: {
      branchPrefix: "agent",
    },
  }) as { worktrees?: { branchPrefix?: string } }

  expect(config.worktrees?.branchPrefix).toBe("agent")
})

test("daemon root config schema accepts managed agent policies", () => {
  const config = buildRootConfigSchema().parse({
    agents: {
      default: "codex-acp",
      managed: {
        "codex-acp": {
          install: "beforeUse",
          update: "daily",
        },
      },
    },
  }) as { agents?: { managed?: Record<string, unknown> } }

  expect(config.agents?.managed?.["codex-acp"]).toEqual({
    install: "beforeUse",
    update: "daily",
  })
})

test("daemon root config schema accepts fixed session profiles", () => {
  const config = buildRootConfigSchema().parse({
    sessionProfiles: {
      "codex-acp": {
        routine: {
          model: "gpt-5.4-mini-low",
          thoughtLevel: "low",
          approvalMode: "default",
        },
      },
    },
  }) as { sessionProfiles?: Record<string, unknown> }

  expect(config.sessionProfiles?.["codex-acp"]).toEqual({
    routine: {
      model: "gpt-5.4-mini-low",
      thoughtLevel: "low",
      approvalMode: "default",
    },
  })
})

test("managed agent policies must declare install or update intent", () => {
  expect(() =>
    buildRootConfigSchema().parse({
      agents: {
        managed: {
          "codex-acp": {},
        },
      },
    }),
  ).toThrow("Managed agents must declare `install`, `update`, or both.")
})

test("generated goddard schema embeds the model schema once under local defs", () => {
  const goddardSchema = buildGeneratedSchemaArtifacts().find(
    (artifact: { name: string }) => artifact.name === "goddard.json",
  )?.jsonSchema as Record<string, unknown>

  expect(goddardSchema.$schema).toBe("https://json-schema.org/draft/2020-12/schema")

  const defs = goddardSchema.$defs as Record<string, Record<string, unknown>>
  expect(defs.ModelConfig).toBeTruthy()
  expect(defs.ModelConfig?.$schema).toBeUndefined()
  expect((defs.SessionTitlesConfig?.properties as Record<string, unknown>)?.generator).toEqual({
    $ref: "#/$defs/ModelConfig",
  })
})

test("editable root config schema embeds every referenced ACP definition", () => {
  const schema = buildEditableRootConfigJsonSchema()
  const references = collectSchemaReferences(schema)
  const defs = schema.$defs as Record<string, unknown>

  expect(references.some((reference) => reference.startsWith("http"))).toBe(false)
  for (const reference of references) {
    expect(reference.startsWith("#/$defs/")).toBe(true)
    expect(defs[reference.slice("#/$defs/".length)]).toBeTruthy()
  }
  expect(defs.ACP_McpServer).toBeTruthy()
})

test("root config merging rejects non-object config fragments before merging", async () => {
  await expect(
    mergeRootConfigLayers(
      {
        agents: "codex-acp",
      },
      undefined,
    ),
  ).rejects.toThrow("agents must be an object.")

  await expect(
    mergeRootConfigLayers(undefined, {
      sessions: "15m",
    }),
  ).rejects.toThrow("sessions must be an object.")
})

test("root config merging keeps managed agents global only", async () => {
  await expect(
    mergeRootConfigLayers(
      {
        agents: {
          default: "global-agent",
          managed: {
            "global-agent": {
              install: "beforeUse",
            },
          },
        },
      },
      {
        agents: {
          default: "local-agent",
        },
      },
    ),
  ).resolves.toMatchObject({
    agents: {
      default: "local-agent",
      managed: {
        "global-agent": {
          install: "beforeUse",
        },
      },
    },
  })

  await expect(
    mergeRootConfigLayers(undefined, {
      agents: {
        managed: {
          "local-agent": {
            install: "beforeUse",
          },
        },
      },
    }),
  ).rejects.toThrow("`agents.managed` is only supported in the global Goddard config.")
})

test("root config merging keeps session profiles global only", async () => {
  await expect(
    mergeRootConfigLayers(
      {
        sessionProfiles: {
          "codex-acp": {
            routine: {
              model: "gpt-5.4-mini-low",
              thoughtLevel: "low",
              approvalMode: "default",
            },
          },
        },
      },
      undefined,
    ),
  ).resolves.toMatchObject({
    sessionProfiles: {
      "codex-acp": {
        routine: {
          model: "gpt-5.4-mini-low",
        },
      },
    },
  })

  await expect(
    mergeRootConfigLayers(undefined, {
      sessionProfiles: {
        "codex-acp": {
          routine: {
            model: "gpt-5.4-mini-low",
            thoughtLevel: "low",
            approvalMode: "default",
          },
        },
      },
    }),
  ).rejects.toThrow("`sessionProfiles` is only supported in the global Goddard config.")
})

function collectSchemaReferences(value: unknown): string[] {
  if (Array.isArray(value)) {
    return value.flatMap(collectSchemaReferences)
  }
  if (typeof value !== "object" || value === null) {
    return []
  }

  const record = value as Record<string, unknown>
  return [
    ...(typeof record.$ref === "string" ? [record.$ref] : []),
    ...Object.values(record).flatMap(collectSchemaReferences),
  ]
}
