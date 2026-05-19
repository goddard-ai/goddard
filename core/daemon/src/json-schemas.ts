import { textModelConfigJsonSchema, transcriptionModelConfigJsonSchema } from "ai-sdk-json-schema"
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
      replaceModelConfigRefs(jsonSchema)
    }
    return { name, jsonSchema }
  })
}

function replaceModelConfigRefs(jsonSchema: Record<string, unknown>) {
  const defs = isObject(jsonSchema.$defs)
    ? (jsonSchema.$defs as Record<string, unknown>)
    : ((jsonSchema.$defs = {}) as Record<string, unknown>)
  defs.ModelConfig = cloneEmbeddedModelSchema(textModelConfigJsonSchema)
  defs.TranscriptionModelConfig = cloneEmbeddedModelSchema(transcriptionModelConfigJsonSchema)

  replacePropertyWithLocalRef(defs, "SessionTitlesConfig", "generator", "#/$defs/ModelConfig")
  replacePropertyWithLocalRef(
    defs,
    "TranscriptionConfig",
    "model",
    "#/$defs/TranscriptionModelConfig",
  )
}

function cloneEmbeddedModelSchema(schema: Record<string, unknown>) {
  const cloned = JSON.parse(JSON.stringify(schema)) as Record<string, unknown>
  delete cloned.$schema
  return cloned
}

function replacePropertyWithLocalRef(
  defs: Record<string, unknown>,
  definitionName: string,
  propertyName: string,
  ref: string,
) {
  const definition = isObject(defs[definitionName])
    ? (defs[definitionName] as Record<string, unknown>)
    : null
  const properties = isObject(definition?.properties)
    ? (definition.properties as Record<string, unknown>)
    : null
  const property = isObject(properties?.[propertyName])
    ? (properties[propertyName] as Record<string, unknown>)
    : null

  if (!property) {
    throw new Error(`Generated RootConfig schema is missing ${definitionName}.${propertyName}.`)
  }

  for (const key of Object.keys(property)) {
    delete property[key]
  }

  property.$ref = ref
}
