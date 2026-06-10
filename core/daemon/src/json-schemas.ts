import { textModelConfigJsonSchema } from "ai-sdk-json-schema"
import { isObject } from "radashi"
import { toJSONSchema, z } from "zod"
import type { ToJSONSchemaParams } from "zod/v4/core"

import { buildRootConfigSchema, registerRootConfigSchemas } from "./config-schema.ts"
import { getDaemonPluginComposition } from "./plugins.ts"

/** Builds generated JSON Schema artifacts for daemon-consumed config files. */
export function buildGeneratedSchemaArtifacts() {
  const acpRegistry = z.registry()
  const rootConfigSchema = buildRootConfigSchema()
  registerRootConfigSchemas(acpRegistry, rootConfigSchema)

  const schemas = [
    { name: "goddard.json", schema: rootConfigSchema },
    ...getDaemonPluginComposition().jsonSchemas,
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
      replaceSessionTitleModelConfig(jsonSchema)
    }
    return { name, jsonSchema }
  })
}

function replaceSessionTitleModelConfig(jsonSchema: Record<string, unknown>) {
  const defs = isObject(jsonSchema.$defs)
    ? (jsonSchema.$defs as Record<string, unknown>)
    : ((jsonSchema.$defs = {}) as Record<string, unknown>)
  const embeddedModelConfig = JSON.parse(JSON.stringify(textModelConfigJsonSchema)) as Record<
    string,
    unknown
  >
  delete embeddedModelConfig.$schema
  defs.ModelConfig = embeddedModelConfig

  const sessionTitlesDefinition = isObject(defs.SessionTitlesConfig)
    ? (defs.SessionTitlesConfig as Record<string, unknown>)
    : null
  const sessionTitlesProperties = isObject(sessionTitlesDefinition?.properties)
    ? (sessionTitlesDefinition.properties as Record<string, unknown>)
    : null
  const generatorProperty = isObject(sessionTitlesProperties?.generator)
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
