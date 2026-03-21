import {
  ActionConfig,
  LoopConfig,
  RootConfig,
  type GoddardActionConfigDocument,
  type GoddardLoopConfigDocument,
  type GoddardRootConfigDocument,
} from "@goddard-ai/schema/config"

export type {
  GoddardActionConfigDocument,
  GoddardLoopConfigDocument,
  GoddardRootConfigDocument,
} from "@goddard-ai/schema/config"


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
  return RootConfig.parse(mergeConfigLayers<GoddardRootConfigDocument>(layers))
}

/** Merges action config layers using later layers as overrides. */
export function mergeActionConfigLayers(
  ...layers: Array<GoddardActionConfigDocument | undefined>
): GoddardActionConfigDocument {
  return ActionConfig.parse(mergeConfigLayers<GoddardActionConfigDocument>(layers))
}

/** Merges loop config layers using later layers as overrides. */
export function mergeLoopConfigLayers(
  ...layers: Array<GoddardLoopConfigDocument | undefined>
): GoddardLoopConfigDocument {
  return LoopConfig.parse(mergeConfigLayers<GoddardLoopConfigDocument>(layers))
}
