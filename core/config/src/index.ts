import { DaemonSessionMetadata } from "@goddard-ai/schema/daemon/session-metadata"
import type { GoddardLoopConfig } from "@goddard-ai/schema/config"
import { AgentDistribution } from "@goddard-ai/schema/session-server"
import { z } from "zod"

export type { GoddardLoopConfig, PiAgentConfig, ThinkingLevel } from "@goddard-ai/schema/config"

// ---------------------------------------------------------------------------
// Models
// ---------------------------------------------------------------------------

/** Canonical model identifiers exposed by the loop configuration package. */
export const Models = {
  Anthropic: {
    Claude37Sonnet: "anthropic/claude-3-7-sonnet-20250219",
    ClaudeSonnet45: "anthropic/claude-sonnet-4-5",
    ClaudeSonnet46: "anthropic/claude-sonnet-4-6",
    ClaudeOpus46: "anthropic/claude-opus-4-6",
  },
  OpenAi: {
    O3Mini: "openai/o3-mini",
    O3Pro: "openai/o3-pro",
    Gpt5Codex: "openai/gpt-5-codex",
    Gpt51Codex: "openai/gpt-5.1-codex",
    Gpt52Codex: "openai/gpt-5.2-codex",
    Gpt53Codex: "openai/gpt-5.3-codex",
  },
} as const

/** Extracts the union of property values from one object type. */
type ValueOf<T> = T[keyof T]

/** Supported model identifiers accepted by loop-facing configuration APIs. */
export type Model = ValueOf<typeof Models.Anthropic> | ValueOf<typeof Models.OpenAi> | (string & {})

const stringRecordSchema = z.record(z.string(), z.string())

const sessionAgentSchema = z.union([z.string().min(1), AgentDistribution])

const sessionConfigSchema = z
  .object({
    agent: sessionAgentSchema.optional(),
    cwd: z.string().min(1).optional(),
    mcpServers: z.array(z.unknown()).optional(),
    systemPrompt: z.string().min(1).optional(),
    env: stringRecordSchema.optional(),
    metadata: DaemonSessionMetadata.optional(),
  })
  .passthrough()

const loopRateLimitsSchema = z
  .object({
    cycleDelay: z.string().min(1).optional(),
    maxOpsPerMinute: z.number().int().positive().optional(),
    maxCyclesBeforePause: z.number().int().positive().optional(),
  })
  .passthrough()

const loopRetriesSchema = z
  .object({
    maxAttempts: z.number().int().positive().optional(),
    initialDelayMs: z.number().int().nonnegative().optional(),
    maxDelayMs: z.number().int().nonnegative().optional(),
    backoffFactor: z.number().positive().optional(),
    jitterRatio: z.number().nonnegative().optional(),
  })
  .passthrough()

/** Schema for persisted action defaults loaded from JSON. */
export const actionConfigSchema = sessionConfigSchema

/** Schema for persisted loop defaults loaded from JSON. */
export const loopConfigSchema = z
  .object({
    session: sessionConfigSchema.optional(),
    rateLimits: loopRateLimitsSchema.optional(),
    retries: loopRetriesSchema.optional(),
  })
  .passthrough()

/** Schema for the shared root config document. */
export const rootConfigSchema = z
  .object({
    actions: actionConfigSchema.optional(),
    loops: loopConfigSchema.optional(),
  })
  .passthrough()

/** Schema for resolved loop rate limits with all required fields present. */
export const resolvedLoopRateLimitsSchema = z.object({
  cycleDelay: z.string().min(1),
  maxOpsPerMinute: z.number().int().positive(),
  maxCyclesBeforePause: z.number().int().positive(),
})

/** Schema for resolved loop retry settings with all required fields present. */
export const resolvedLoopRetriesSchema = z.object({
  maxAttempts: z.number().int().positive(),
  initialDelayMs: z.number().int().nonnegative(),
  maxDelayMs: z.number().int().nonnegative(),
  backoffFactor: z.number().positive(),
  jitterRatio: z.number().nonnegative(),
})

/** Schema for a fully resolved loop config document. */
export const resolvedLoopConfigSchema = z.object({
  session: sessionConfigSchema.extend({
    agent: sessionAgentSchema,
    cwd: z.string().min(1),
    mcpServers: z.array(z.unknown()),
  }),
  rateLimits: resolvedLoopRateLimitsSchema,
  retries: resolvedLoopRetriesSchema,
})

/** A persisted action config document layered before runtime overrides. */
export type GoddardActionConfigDocument = z.infer<typeof actionConfigSchema>

/** A persisted loop config document layered before runtime overrides. */
export type GoddardLoopConfigDocument = z.infer<typeof loopConfigSchema>

/** A persisted root config document for shared action and loop defaults. */
export type GoddardRootConfigDocument = z.infer<typeof rootConfigSchema>

/** A resolved loop rate-limit block ready for runtime execution. */
export type GoddardLoopRateLimitsConfig = z.infer<typeof resolvedLoopRateLimitsSchema>

/** A resolved loop retry block ready for runtime execution. */
export type GoddardLoopRetriesConfig = z.infer<typeof resolvedLoopRetriesSchema>

/** A resolved loop config with all JSON-safe required fields present. */
export type ResolvedGoddardLoopConfigDocument = z.infer<typeof resolvedLoopConfigSchema>

function isPlainObject(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value)
}

function mergeValue(baseValue: unknown, overrideValue: unknown): unknown {
  if (overrideValue === undefined) {
    return baseValue
  }

  if (Array.isArray(overrideValue)) {
    return [...overrideValue]
  }

  if (!isPlainObject(overrideValue)) {
    return overrideValue
  }

  const baseObject = isPlainObject(baseValue) ? baseValue : {}
  const merged: Record<string, unknown> = { ...baseObject }

  for (const [key, value] of Object.entries(overrideValue)) {
    merged[key] = mergeValue(baseObject[key], value)
  }

  return merged
}

function mergeConfigLayers<T extends Record<string, unknown>>(layers: Array<T | undefined>): T {
  let merged: Record<string, unknown> = {}

  for (const layer of layers) {
    if (!layer) {
      continue
    }

    merged = mergeValue(merged, layer) as Record<string, unknown>
  }

  return merged as T
}

/** Merges root config layers using later layers as overrides. */
export function mergeRootConfigLayers(
  ...layers: Array<GoddardRootConfigDocument | undefined>
): GoddardRootConfigDocument {
  return rootConfigSchema.parse(mergeConfigLayers<GoddardRootConfigDocument>(layers))
}

/** Merges action config layers using later layers as overrides. */
export function mergeActionConfigLayers(
  ...layers: Array<GoddardActionConfigDocument | undefined>
): GoddardActionConfigDocument {
  return actionConfigSchema.parse(mergeConfigLayers<GoddardActionConfigDocument>(layers))
}

/** Merges loop config layers using later layers as overrides. */
export function mergeLoopConfigLayers(
  ...layers: Array<GoddardLoopConfigDocument | undefined>
): GoddardLoopConfigDocument {
  return loopConfigSchema.parse(mergeConfigLayers<GoddardLoopConfigDocument>(layers))
}

/** Returns loop configuration unchanged so callers can author typed config files. */
export function defineConfig(config: GoddardLoopConfig): GoddardLoopConfig {
  return config
}
