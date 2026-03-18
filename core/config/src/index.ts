import { z } from "zod"

// A model identifier accepted by persisted and runtime Goddard configuration.
export type Model =
  | "anthropic/claude-3-7-sonnet-20250219"
  | "anthropic/claude-sonnet-4-5"
  | "anthropic/claude-sonnet-4-6"
  | "anthropic/claude-opus-4-6"
  | "openai/o3-mini"
  | "openai/o3-pro"
  | "openai/gpt-5-codex"
  | "openai/gpt-5.1-codex"
  | "openai/gpt-5.2-codex"
  | "openai/gpt-5.3-codex"
  | (string & {})

// ---------------------------------------------------------------------------
// Primitive schemas
// ---------------------------------------------------------------------------

const thinkingLevelSchema = z.enum(["off", "minimal", "low", "medium", "high", "xhigh"])

// A persisted thinking level for an agent-backed session.
export type ThinkingLevel = z.infer<typeof thinkingLevelSchema>

const stringRecordSchema = z.record(z.string(), z.string())
const metadataSchema = z.record(z.string(), z.unknown())

const agentDistributionSchema = z
  .object({
    type: z.enum(["binary", "npx", "uvx"]),
    package: z.string().min(1).optional(),
    cmd: z.string().min(1).optional(),
    args: z.array(z.string()).optional(),
  })
  .passthrough()

const sessionAgentSchema = z.union([z.string().min(1), agentDistributionSchema])

const sessionConfigSchema = z
  .object({
    agent: sessionAgentSchema.optional(),
    cwd: z.string().min(1).optional(),
    mcpServers: z.array(z.unknown()).optional(),
    systemPrompt: z.string().min(1).optional(),
    env: stringRecordSchema.optional(),
    metadata: metadataSchema.optional(),
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

export const actionConfigSchema = sessionConfigSchema

export const loopConfigSchema = z
  .object({
    session: sessionConfigSchema.optional(),
    rateLimits: loopRateLimitsSchema.optional(),
    retries: loopRetriesSchema.optional(),
  })
  .passthrough()

export const rootConfigSchema = z
  .object({
    actions: actionConfigSchema.optional(),
    loops: loopConfigSchema.optional(),
  })
  .passthrough()

export const resolvedLoopRateLimitsSchema = z.object({
  cycleDelay: z.string().min(1),
  maxOpsPerMinute: z.number().int().positive(),
  maxCyclesBeforePause: z.number().int().positive(),
})

export const resolvedLoopRetriesSchema = z.object({
  maxAttempts: z.number().int().positive(),
  initialDelayMs: z.number().int().nonnegative(),
  maxDelayMs: z.number().int().nonnegative(),
  backoffFactor: z.number().positive(),
  jitterRatio: z.number().nonnegative(),
})

export const resolvedLoopConfigSchema = z.object({
  session: sessionConfigSchema.extend({
    agent: sessionAgentSchema,
    cwd: z.string().min(1),
    mcpServers: z.array(z.unknown()),
  }),
  rateLimits: resolvedLoopRateLimitsSchema,
  retries: resolvedLoopRetriesSchema,
})

// A persisted action config document layered with root defaults before runtime overrides.
export type GoddardActionConfigDocument = z.infer<typeof actionConfigSchema>

// A persisted loop config document layered with root defaults before runtime overrides.
export type GoddardLoopConfigDocument = z.infer<typeof loopConfigSchema>

// A persisted root config document for shared action and loop defaults.
export type GoddardRootConfigDocument = z.infer<typeof rootConfigSchema>

// A resolved loop rate-limit block ready to be converted into runtime params.
export type GoddardLoopRateLimitsConfig = z.infer<typeof resolvedLoopRateLimitsSchema>

// A resolved loop retry block ready to be converted into runtime params.
export type GoddardLoopRetriesConfig = z.infer<typeof resolvedLoopRetriesSchema>

// A resolved loop config with all JSON-safe required fields present.
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

export function mergeRootConfigLayers(
  ...layers: Array<GoddardRootConfigDocument | undefined>
): GoddardRootConfigDocument {
  return rootConfigSchema.parse(mergeConfigLayers<GoddardRootConfigDocument>(layers))
}

export function mergeActionConfigLayers(
  ...layers: Array<GoddardActionConfigDocument | undefined>
): GoddardActionConfigDocument {
  return actionConfigSchema.parse(mergeConfigLayers<GoddardActionConfigDocument>(layers))
}

export function mergeLoopConfigLayers(
  ...layers: Array<GoddardLoopConfigDocument | undefined>
): GoddardLoopConfigDocument {
  return loopConfigSchema.parse(mergeConfigLayers<GoddardLoopConfigDocument>(layers))
}
