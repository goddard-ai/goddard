import type {
  DaemonEventEnvelope,
  EventBus,
  EventDefinition,
  EventLogMetadata,
} from "./contracts.ts"

type Listener<TPayload = unknown> = (payload: TPayload) => void | Promise<void>
type Observer = (event: DaemonEventEnvelope) => void | Promise<void>

export type EventDefinitionOptions = EventLogMetadata

export type DaemonEventFilter = {
  readonly names?: readonly string[]
  readonly where?: readonly DaemonEventPropertyFilter[]
}

export type DaemonEventPropertyFilter = {
  readonly path: string
  readonly equals: unknown
}

/** Declares one daemon plugin event payload type without adding runtime behavior. */
export function event<TPayload>(options: EventDefinitionOptions = {}): EventDefinition<TPayload> {
  return Object.keys(options).length > 0 ? { log: options } : {}
}

/** Creates the in-process event bus shared by one daemon plugin composition. */
export function createDaemonEventBus(
  definitions: Record<string, EventDefinition<unknown>> = {},
): EventBus<Record<string, EventDefinition<unknown>>> {
  const cache = new Map<string, Set<Listener>>()
  const observers = new Set<Observer>()

  return {
    on(eventName, listener) {
      const listeners = cache.get(eventName) ?? new Set<Listener>()
      listeners.add(listener)
      cache.set(eventName, listeners)

      return () => {
        listeners.delete(listener)
        if (listeners.size === 0) {
          cache.delete(eventName)
        }
      }
    },
    observe(listener) {
      observers.add(listener as Observer)

      return () => {
        observers.delete(listener as Observer)
      }
    },
    async emit(eventName, payload) {
      const envelope: DaemonEventEnvelope = {
        id: globalThis.crypto.randomUUID(),
        at: new Date().toISOString(),
        name: eventName,
        payload,
        log: definitions[eventName]?.log,
      }

      for (const observer of [...observers]) {
        await observer(envelope)
      }

      const listeners = cache.get(eventName)
      if (!listeners) {
        return
      }
      // oxlint-disable-next-line unicorn/no-useless-spread
      for (const listener of [...listeners]) {
        await listener(payload)
      }
    },
  }
}

/** Returns whether one event envelope satisfies an exact payload-property subscription filter. */
export function matchesDaemonEventFilter(
  event: Pick<DaemonEventEnvelope, "name" | "payload">,
  filter: DaemonEventFilter = {},
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
  return typeof value === "object" && value !== null && !Array.isArray(value)
}
