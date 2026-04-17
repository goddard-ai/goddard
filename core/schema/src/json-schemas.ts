import { textModelConfigJsonSchema } from "ai-sdk-json-schema"
import { toJSONSchema, z } from "zod"
import type { ToJSONSchemaParams } from "zod/v4/core"

import { ActionConfig, LoopConfig, UserConfig, registerConfigSchemas } from "./config.ts"

function isJsonObject(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value)
}

/** Builds the generated JSON Schema artifacts shared by config docs and tests. */
export function buildGeneratedSchemaArtifacts() {
  const acpRegistry = z.registry()
  registerConfigSchemas(acpRegistry)

  const schemas = [
    { name: "goddard.json", schema: UserConfig },
    { name: "action.json", schema: ActionConfig },
    { name: "loop.json", schema: LoopConfig },
  ].map(({ name, schema }) => ({
    name,
    schema: schema.extend({
      $schema: z.string(),
    }),
  }))

  const schemaParams: ToJSONSchemaParams = {
    target: "draft-2020-12",
    io: "input",
    override(ctx) {
      const { id } = ctx.jsonSchema
      if (id && acpRegistry.has(ctx.zodSchema)) {
        for (const key in ctx.jsonSchema) {
          delete ctx.jsonSchema[key]
        }
        ctx.jsonSchema.$ref = `https://raw.githubusercontent.com/agentclientprotocol/agent-client-protocol/main/schema/schema.json#/$defs/${id}`
      }
    },
  }

  return schemas.map(({ name, schema }) => {
    const jsonSchema = toJSONSchema(schema, schemaParams) as Record<string, unknown>
    if (name === "goddard.json") {
      const defs = isJsonObject(jsonSchema.$defs)
        ? (jsonSchema.$defs as Record<string, unknown>)
        : ((jsonSchema.$defs = {}) as Record<string, unknown>)
      const embeddedModelConfig = JSON.parse(JSON.stringify(textModelConfigJsonSchema)) as Record<
        string,
        unknown
      >
      delete embeddedModelConfig.$schema
      defs.ModelConfig = embeddedModelConfig

      const sessionTitlesDefinition = isJsonObject(defs.SessionTitlesConfig)
        ? (defs.SessionTitlesConfig as Record<string, unknown>)
        : null
      const sessionTitlesProperties = isJsonObject(sessionTitlesDefinition?.properties)
        ? (sessionTitlesDefinition.properties as Record<string, unknown>)
        : null
      const generatorProperty = isJsonObject(sessionTitlesProperties?.generator)
        ? (sessionTitlesProperties.generator as Record<string, unknown>)
        : null

      if (!generatorProperty) {
        throw new Error("Generated RootConfig schema is missing sessionTitles.generator.")
      }

      for (const key of Object.keys(generatorProperty)) {
        delete generatorProperty[key]
      }

      generatorProperty.$ref = "#/$defs/ModelConfig"
    }
    return { name, jsonSchema }
  })
}
