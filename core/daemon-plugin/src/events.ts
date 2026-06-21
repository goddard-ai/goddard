import type { EventBus, EventDefinition } from "./contracts.ts"

type Listener = (payload: unknown) => void | Promise<void>

/** Declares one daemon plugin event payload type without adding runtime behavior. */
export function event<TPayload>(): EventDefinition<TPayload> {
  return {}
}

/** Creates the in-process event bus shared by one daemon plugin composition. */
export function createDaemonEventBus(): EventBus<Record<string, EventDefinition<unknown>>> {
  const cache = new Map<string, Set<Listener>>()

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
    async emit(eventName, payload) {
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
