import acpJsonSchema from "@agentclientprotocol/sdk/schema/schema.json" with { type: "json" }
import { textModelConfigJsonSchema } from "ai-sdk-json-schema"
import { isObject } from "radashi"
import { toJSONSchema, z } from "zod"
import type { ToJSONSchemaParams } from "zod/v4/core"

import { buildRootConfigSchema, registerRootConfigSchemas } from "./config-schema.ts"
import { getDaemonPluginComposition } from "./plugins.ts"

const acpSchemaUrl =
  "https://raw.githubusercontent.com/agentclientprotocol/agent-client-protocol/main/schema/schema.json"
const embeddedAcpDefinitionPrefix = "ACP_"

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

/** Builds the composed root schema used to render editable user configuration controls. */
export function buildEditableRootConfigJsonSchema() {
  const artifact = buildGeneratedSchemaArtifacts().find(({ name }) => name === "goddard.json")
  if (!artifact) {
    throw new Error("Generated root configuration schema is unavailable.")
  }

  const jsonSchema = structuredClone(artifact.jsonSchema)
  if (isObject(jsonSchema.properties)) {
    delete (jsonSchema.properties as Record<string, unknown>).$schema
  }
  if (Array.isArray(jsonSchema.required)) {
    const required = jsonSchema.required.filter((key) => key !== "$schema")
    if (required.length > 0) {
      jsonSchema.required = required
    } else {
      delete jsonSchema.required
    }
  }

  inlineAcpDefinitions(jsonSchema)

  return jsonSchema
}

function inlineAcpDefinitions(jsonSchema: Record<string, unknown>) {
  const targetDefs = isObject(jsonSchema.$defs)
    ? (jsonSchema.$defs as Record<string, unknown>)
    : ((jsonSchema.$defs = {}) as Record<string, unknown>)
  const sourceDefs = acpJsonSchema.$defs as Record<string, unknown>
  const pendingDefinitions = new Set<string>()

  rewriteAcpReferences(jsonSchema, `${acpSchemaUrl}#/$defs/`, pendingDefinitions)

  for (const name of pendingDefinitions) {
    const sourceDefinition = sourceDefs[name]
    if (!sourceDefinition) {
      throw new Error(`ACP schema definition ${name} is unavailable.`)
    }

    const definition = structuredClone(sourceDefinition)
    rewriteAcpReferences(definition, "#/$defs/", pendingDefinitions)
    targetDefs[`${embeddedAcpDefinitionPrefix}${name}`] = definition
  }
}

function rewriteAcpReferences(
  value: unknown,
  referencePrefix: string,
  pendingDefinitions: Set<string>,
) {
  if (Array.isArray(value)) {
    for (const item of value) {
      rewriteAcpReferences(item, referencePrefix, pendingDefinitions)
    }
    return
  }

  if (!isObject(value)) {
    return
  }

  const record = value as Record<string, unknown>
  if (typeof record.$ref === "string" && record.$ref.startsWith(referencePrefix)) {
    const name = record.$ref.slice(referencePrefix.length)
    pendingDefinitions.add(name)
    record.$ref = `#/$defs/${embeddedAcpDefinitionPrefix}${name}`
  }

  for (const child of Object.values(record)) {
    rewriteAcpReferences(child, referencePrefix, pendingDefinitions)
  }
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
