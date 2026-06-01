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

export function mergeConfigLayers<T extends Record<string, unknown>>(
  layers: Array<T | undefined>,
): T {
  let merged: Record<string, unknown> = {}

  for (const layer of layers) {
    if (!layer) {
      continue
    }

    merged = mergeValue(merged, layer) as Record<string, unknown>
  }

  return merged as T
}

export function selectLast<T, R>(
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
