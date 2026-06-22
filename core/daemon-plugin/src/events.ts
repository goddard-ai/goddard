import { isObject } from "radashi"

import type {
  DaemonEventEnvelope,
  DaemonEventFilter,
  DaemonEventPropertyFilter,
  DaemonEventSubscriptionEvent,
  EventBus,
  EventDefinition,
  EventLogMetadata,
} from "./contracts.ts"

type Listener<TPayload = unknown> = (payload: TPayload) => void | Promise<void>
type Observer = (event: DaemonEventEnvelope) => void | Promise<void>
type SubscriptionObserver = (event: DaemonEventSubscriptionEvent) => void | Promise<void>

export type EventDefinitionOptions = EventLogMetadata
export type { DaemonEventFilter, DaemonEventPropertyFilter }

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
  const subscriptionObservers = new Set<SubscriptionObserver>()

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
    onSubscription(listener) {
      subscriptionObservers.add(listener as SubscriptionObserver)

      return () => {
        subscriptionObservers.delete(listener as SubscriptionObserver)
      }
    },
    stream(filter, signal) {
      const queue: DaemonEventEnvelope[] = []
      let wake: (() => void) | undefined
      let started = false
      let closed = false
      const listener = (event: DaemonEventEnvelope) => {
        if (closed) {
          return
        }
        if (!matchesDaemonEventFilter(event, filter)) {
          return
        }
        queue.push(event)
        wake?.()
      }
      const abort = () => {
        void close()
      }

      async function start() {
        if (started) {
          return
        }
        started = true
        await notifySubscriptionObservers(subscriptionObservers, {
          state: "started",
          filter,
        })
        observers.add(listener)
        signal.addEventListener("abort", abort)
      }

      async function close() {
        if (closed) {
          return
        }
        closed = true
        signal.removeEventListener("abort", abort)
        if (started) {
          observers.delete(listener)
        }
        wake?.()
        wake = undefined
        if (started) {
          await notifySubscriptionObservers(subscriptionObservers, {
            state: "ended",
            filter,
          })
        }
      }

      return {
        [Symbol.asyncIterator]() {
          return {
            async next() {
              await start()
              if (closed || signal.aborted) {
                await close()
                return { done: true, value: undefined }
              }

              while (queue.length === 0) {
                await new Promise<void>((resolve) => {
                  wake = resolve
                })
                wake = undefined

                if (closed || signal.aborted) {
                  await close()
                  return { done: true, value: undefined }
                }
              }

              return { done: false, value: queue.shift()! }
            },
            async return() {
              await close()
              return { done: true, value: undefined }
            },
          }
        },
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

async function notifySubscriptionObservers(
  observers: Set<SubscriptionObserver>,
  event: DaemonEventSubscriptionEvent,
) {
  for (const observer of [...observers]) {
    await observer(event)
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
    if (!isObject(current) || !(segment in current)) {
      return undefined
    }
    current = current[segment as keyof typeof current]
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

  if (!isObject(actual) || !isObject(expected)) {
    return false
  }

  const actualKeys = Object.keys(actual)
  const expectedKeys = Object.keys(expected)
  if (actualKeys.length !== expectedKeys.length) {
    return false
  }

  return expectedKeys.every(
    (key) =>
      key in actual &&
      isExactMatch(actual[key as keyof typeof actual], expected[key as keyof typeof expected]),
  )
}
