import type { EventBus, EventDefinition } from "./contracts.ts"

type Listener = (payload: unknown) => void | Promise<void>

/** Declares one daemon plugin event payload type without adding runtime behavior. */
export function event<TPayload>(): EventDefinition<TPayload> {
  return {}
}

/** Creates the in-process event bus shared by one daemon plugin composition. */
export function createDaemonEventBus(): EventBus<Record<string, EventDefinition<unknown>>> {
  const listeners = new Map<string, Set<Listener>>()

  return {
    on(eventName, listener) {
      const eventListeners = listeners.get(eventName) ?? new Set<Listener>()
      eventListeners.add(listener as Listener)
      listeners.set(eventName, eventListeners)

      return () => {
        eventListeners.delete(listener as Listener)
        if (eventListeners.size === 0) {
          listeners.delete(eventName)
        }
      }
    },
    async emit(eventName, payload) {
      for (const listener of [...(listeners.get(eventName) ?? [])]) {
        await listener(payload)
      }
    },
  }
}
