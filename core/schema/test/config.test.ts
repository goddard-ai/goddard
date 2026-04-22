import { expect, test } from "bun:test"

import { UserConfig } from "../src/config.ts"
import { buildGeneratedSchemaArtifacts } from "../src/json-schemas.ts"

test("UserConfig accepts session title generator model config", () => {
  const config = UserConfig.parse({
    sessionTitles: {
      generator: {
        provider: "openai",
        model: "gpt-4.1-mini",
      },
    },
  })

  expect(config.sessionTitles?.generator).toEqual({
    provider: "openai",
    model: "gpt-4.1-mini",
  })
})

test("UserConfig accepts a global daemon port override", () => {
  const config = UserConfig.parse({
    daemon: {
      port: 49828,
    },
  })

  expect(config.daemon?.port).toBe(49828)
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
