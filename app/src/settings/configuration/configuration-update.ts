import type { UpdateUserConfigRequest, UserConfigDocument } from "@goddard-ai/sdk"

type JsonPath = Array<string | number>

/** Derives the narrowest field or collection update represented by the next form document. */
export function deriveUserConfigUpdate(
  current: UserConfigDocument,
  next: UserConfigDocument,
): UpdateUserConfigRequest | null {
  const changedPaths = collectChangedPaths(current, next)
  if (changedPaths.length === 0) {
    return null
  }

  const path = findCommonPath(changedPaths)
  if (path.length === 0) {
    return null
  }

  const pointer = encodeJsonPointer(path)
  const nextValue = readPath(next, path)
  if (!nextValue.exists) {
    return {
      operation: "remove",
      path: pointer,
    }
  }

  return {
    operation: "set",
    path: pointer,
    value: nextValue.value,
  }
}

function collectChangedPaths(current: unknown, next: unknown, path: JsonPath = []): JsonPath[] {
  if (isDeepEqual(current, next)) {
    return []
  }

  if (Array.isArray(current) || Array.isArray(next)) {
    return [path]
  }

  if (isRecord(current) && isRecord(next)) {
    const keys = new Set([...jsonObjectKeys(current), ...jsonObjectKeys(next)])
    return [...keys].flatMap((key) =>
      collectChangedPaths(readJsonProperty(current, key), readJsonProperty(next, key), [
        ...path,
        key,
      ]),
    )
  }

  return [path]
}

function findCommonPath(paths: JsonPath[]) {
  const [first, ...rest] = paths as [JsonPath, ...JsonPath[]]
  return first.filter((segment, index) => rest.every((path) => path[index] === segment))
}

function readPath(document: UserConfigDocument, path: JsonPath) {
  let value: unknown = document

  for (const segment of path) {
    if (Array.isArray(value) && typeof segment === "number" && segment < value.length) {
      value = value[segment]
      continue
    }
    if (isRecord(value) && typeof segment === "string" && hasJsonProperty(value, segment)) {
      value = value[segment]
      continue
    }
    return { exists: false as const }
  }

  return { exists: true as const, value }
}

function isDeepEqual(left: unknown, right: unknown): boolean {
  if (Object.is(left, right)) {
    return true
  }
  if (Array.isArray(left) && Array.isArray(right)) {
    return (
      left.length === right.length && left.every((value, index) => isDeepEqual(value, right[index]))
    )
  }
  if (isRecord(left) && isRecord(right)) {
    const leftKeys = jsonObjectKeys(left)
    const rightKeys = jsonObjectKeys(right)
    return (
      leftKeys.length === rightKeys.length &&
      leftKeys.every((key) => hasJsonProperty(right, key) && isDeepEqual(left[key], right[key]))
    )
  }
  return false
}

function readJsonProperty(document: Record<string, unknown>, key: string) {
  return hasJsonProperty(document, key) ? document[key] : undefined
}

function jsonObjectKeys(document: Record<string, unknown>) {
  return Object.keys(document).filter((key) => document[key] !== undefined)
}

function hasJsonProperty(document: Record<string, unknown>, key: string) {
  return Object.hasOwn(document, key) && document[key] !== undefined
}

function encodeJsonPointer(path: JsonPath) {
  return `/${path
    .map(String)
    .map((segment) => segment.replaceAll("~", "~0").replaceAll("/", "~1"))
    .join("/")}`
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value)
}
