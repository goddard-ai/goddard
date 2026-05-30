import { mergeConfigLayers } from "@goddard-ai/config"
import { type ConfigDefinition } from "@goddard-ai/daemon-plugin"
import { AgentDistribution } from "@goddard-ai/schema/agent-distribution"
import { DaemonConfig, registerConfigSchemas } from "@goddard-ai/schema/config"
import { z } from "zod"

import { getDaemonPluginComposition } from "./plugins.ts"

/** Core daemon-owned root config sections that are not owned by feature plugins. */
export const CoreRootConfig = {
  daemon: DaemonConfig.optional().describe("Default settings for the local daemon server."),
  registry: z
    .record(z.string(), AgentDistribution)
    .optional()
    .describe("Custom registry of ACP agent distributions."),
} as const

type RootConfigLayer = Record<string, unknown>

/** Builds the effective root config schema for this daemon build. */
export function buildRootConfigSchema() {
  return z
    .strictObject({
      ...CoreRootConfig,
      ...Object.fromEntries(
        Object.entries(getDaemonPluginComposition().config).map(([key, definition]) => [
          key,
          definition.schema.optional(),
        ]),
      ),
    })
    .describe("Shared root config document loaded from local and global JSON files.")
}

/** Registers root config schemas used when generating the daemon config JSON Schema. */
export function registerRootConfigSchemas(
  acpRegistry: z.core.$ZodRegistry,
  rootConfigSchema = buildRootConfigSchema(),
) {
  registerConfigSchemas(acpRegistry)
  z.globalRegistry.add(rootConfigSchema, { id: "RootConfig" })

  for (const [key, definition] of Object.entries(getDaemonPluginComposition().config)) {
    z.globalRegistry.add(definition.schema, { id: getConfigSchemaId(key) })
  }
}

/** Merges parsed root config layers using each config owner namespace's merge semantics. */
export async function mergeRootConfigLayers(
  user: RootConfigLayer | undefined,
  project: RootConfigLayer | undefined,
) {
  const config: Record<string, unknown> = {}

  const daemon = user?.daemon
  if (daemon !== undefined) {
    config.daemon = daemon
  }

  const registry = mergeConfigLayers([
    user?.registry as Record<string, unknown> | undefined,
    project?.registry as Record<string, unknown> | undefined,
  ])
  if (Object.keys(registry).length > 0) {
    config.registry = registry
  }

  for (const [key, definition] of Object.entries(getDaemonPluginComposition().config)) {
    const value = await resolveConfigValue(definition, user?.[key], project?.[key])
    if (value !== undefined) {
      config[key] = value
    }
  }

  return config
}

async function resolveConfigValue(definition: ConfigDefinition, user: unknown, project: unknown) {
  if (definition.resolve) {
    return definition.resolve({
      user,
      project,
    } as Parameters<NonNullable<typeof definition.resolve>>[0])
  }

  const merged = mergeConfigLayers([
    user as Record<string, unknown> | undefined,
    project as Record<string, unknown> | undefined,
  ])

  if (Object.keys(merged).length === 0 && user === undefined && project === undefined) {
    return undefined
  }

  return definition.schema.parse(merged)
}

function getConfigSchemaId(key: string) {
  if (key === "actions") {
    return "ActionConfig"
  }
  if (key === "loops") {
    return "LoopConfig"
  }

  return `${toPascalCase(key)}Config`
}

function toPascalCase(value: string) {
  return value
    .split(/[^a-zA-Z0-9]+/)
    .filter(Boolean)
    .map((part) => `${part[0]?.toUpperCase() ?? ""}${part.slice(1)}`)
    .join("")
}
