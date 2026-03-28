import { ActionConfig, LoopConfig, UserConfig } from "@goddard-ai/schema/config"
import { isPlainObject } from "radashi"

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

  const baseObject = isPlainObject(baseValue) ? (baseValue as Record<string, unknown>) : {}
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

function selectLast<T, R>(
  values: ReadonlyArray<T>,
  predicate: (value: T, index: number) => R | undefined,
): R | undefined {
  for (let index = values.length - 1; index >= 0; index -= 1) {
    const result = predicate(values[index], index)
    if (result !== undefined) {
      return result
    }
  }

  return undefined
}

/** Merges root config layers using later layers as overrides. */
export function mergeRootConfigLayers(...layers: Array<UserConfig | undefined>): UserConfig {
  const merged = mergeConfigLayers<UserConfig>(layers)

  return UserConfig.parse({
    ...merged,
    session: selectLast(layers, (layer) => layer?.session),
    actions: mergeActionConfigLayers(...layers.map((layer) => layer?.actions)),
    loops: mergeLoopConfigLayers(...layers.map((layer) => layer?.loops)),
    registry: layers.reduce(
      (acc, layer) => {
        if (layer?.registry) {
          return { ...acc, ...layer.registry }
        }
        return acc
      },
      undefined as UserConfig["registry"],
    ),
  })
}

/** Merges action config layers using later layers as overrides. */
export function mergeActionConfigLayers(...layers: Array<ActionConfig | undefined>): ActionConfig {
  const merged = mergeConfigLayers<ActionConfig>(layers)

  return ActionConfig.parse({
    ...merged,
    session: selectLast(layers, (layer) => layer?.session),
  })
}

/** Merges loop config layers using later layers as overrides. */
export function mergeLoopConfigLayers(...layers: Array<LoopConfig | undefined>): LoopConfig {
  const merged = mergeConfigLayers<LoopConfig>(layers)

  return LoopConfig.parse({
    ...merged,
    session: selectLast(layers, (layer) => layer?.session),
  })
}

export * from "./agent-resolver.js"
