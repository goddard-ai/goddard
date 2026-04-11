import type { InferStreamPayload } from "@goddard-ai/ipc"
import type { daemonIpcSchema } from "@goddard-ai/schema/daemon-ipc"
import type {
  DaemonResetSubscriptionsInput,
  DaemonStreamTargetInput,
  DaemonSubscribeInput,
  DaemonUnsubscribeInput,
} from "./desktop-rpc.ts"
import type { DaemonStreamEventDetail, DaemonStreamName } from "./global-event-hub.ts"

type DaemonSchema = typeof daemonIpcSchema

type CoordinatorDeps = {
  webviewId: number
  nextSubscriptionId?: () => string
  onUnsubscribeError?: (error: unknown) => void
  resetSubscriptions: (input: DaemonResetSubscriptionsInput) => Promise<unknown>
  subscribe: (input: DaemonSubscribeInput) => Promise<unknown>
  unsubscribe: (input: DaemonUnsubscribeInput) => Promise<unknown>
}

/** Stable browser-side daemon stream subscription interface shared across app modules. */
export interface DaemonSubscriptionCoordinator {
  /** Routes one forwarded Bun-host daemon stream payload to its registered callback. */
  dispatchEvent(detail: DaemonStreamEventDetail): void

  /** Clears every daemon stream subscription still owned by the current webview. */
  reset(): Promise<void>

  /** Opens one daemon stream subscription and returns an idempotent unsubscribe closure. */
  subscribe<TName extends DaemonStreamName>(
    target: DaemonStreamTargetInput<TName>,
    onMessage: (payload: InferStreamPayload<DaemonSchema, TName>) => void,
  ): Promise<() => void>
}

/**
 * Coordinates browser-side daemon stream subscriptions so reload cleanup, callback routing, and
 * idempotent unsubscribe behavior stay consistent across all stream consumers.
 */
export function createDaemonSubscriptionCoordinator(
  deps: CoordinatorDeps,
): DaemonSubscriptionCoordinator {
  const callbacks = new Map<string, (payload: unknown) => void>()
  let resetPromise: Promise<void> | null = null

  function nextSubscriptionId() {
    return deps.nextSubscriptionId?.() ?? crypto.randomUUID()
  }

  function reset() {
    if (resetPromise) {
      return resetPromise
    }

    resetPromise = deps
      .resetSubscriptions({
        webviewId: deps.webviewId,
      })
      .then(() => {})
      .catch((error) => {
        resetPromise = null
        throw error
      })

    return resetPromise
  }

  async function subscribe<TName extends DaemonStreamName>(
    target: DaemonStreamTargetInput<TName>,
    onMessage: (payload: InferStreamPayload<DaemonSchema, TName>) => void,
  ) {
    await reset()

    const subscriptionId = nextSubscriptionId()
    let active = true

    callbacks.set(subscriptionId, onMessage as (payload: unknown) => void)

    try {
      await deps.subscribe({
        webviewId: deps.webviewId,
        subscriptionId,
        target,
      })
    } catch (error) {
      callbacks.delete(subscriptionId)
      throw error
    }

    return () => {
      if (!active) {
        return
      }

      active = false
      callbacks.delete(subscriptionId)

      void deps.unsubscribe({ subscriptionId }).catch((error) => {
        deps.onUnsubscribeError?.(error)
      })
    }
  }

  function dispatchEvent(detail: DaemonStreamEventDetail) {
    callbacks.get(detail.subscriptionId)?.(detail.payload)
  }

  return {
    dispatchEvent,
    reset,
    subscribe,
  }
}
