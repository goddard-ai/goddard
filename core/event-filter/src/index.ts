export type EventEnvelopeFilter = {
  readonly names?: readonly string[]
  readonly where?: readonly EventEnvelopePropertyFilter[]
}

export type EventEnvelopePropertyFilter = {
  readonly path: string
  readonly equals: unknown
}

/** Returns whether one event envelope satisfies an exact payload-property subscription filter. */
export function matchesEventEnvelopeFilter(
  event: { readonly name: string; readonly payload: unknown },
  filter: EventEnvelopeFilter = {},
) {
  if (filter.names && filter.names.length > 0 && !filter.names.includes(event.name)) {
    return false
  }

  for (const condition of filter.where ?? []) {
    if (!isExactMatch(readPayloadPath(event.payload, condition.path), condition.equals)) {
      return false
    }
  }

  return true
}

function readPayloadPath(payload: unknown, path: string) {
  if (path.length === 0) {
    return undefined
  }

  let current = payload
  for (const segment of path.split(".")) {
    if (!isRecord(current) || !(segment in current)) {
      return undefined
    }
    current = current[segment]
  }
  return current
}

function isExactMatch(actual: unknown, expected: unknown): boolean {
  if (Object.is(actual, expected)) {
    return true
  }

  if (Array.isArray(actual) || Array.isArray(expected)) {
    if (!Array.isArray(actual) || !Array.isArray(expected) || actual.length !== expected.length) {
      return false
    }
    return actual.every((value, index) => isExactMatch(value, expected[index]))
  }

  if (!isRecord(actual) || !isRecord(expected)) {
    return false
  }

  const actualKeys = Object.keys(actual)
  const expectedKeys = Object.keys(expected)
  if (actualKeys.length !== expectedKeys.length) {
    return false
  }

  return expectedKeys.every((key) => key in actual && isExactMatch(actual[key], expected[key]))
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return !!value && typeof value === "object"
}
