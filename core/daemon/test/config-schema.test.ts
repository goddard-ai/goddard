import { expect, test } from "bun:test"

import { buildRootConfigSchema, mergeRootConfigLayers } from "../src/config-schema.ts"
import { buildGeneratedSchemaArtifacts } from "../src/json-schemas.ts"

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
