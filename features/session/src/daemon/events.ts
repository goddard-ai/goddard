/** Typed lifecycle events emitted by the session feature for downstream daemon plugins. */
import type { InboxHeadline, InboxItem, InboxScope } from "@goddard-ai/inbox/schema"
import type { DaemonSessionId } from "@goddard-ai/schema/common/params"

type MaybePromise<T> = T | Promise<T>

type EventListener<TPayload, TResult = void> = (payload: TPayload) => MaybePromise<TResult>

type EventPayload<TEvents, TName extends keyof TEvents> = TEvents[TName] extends (
  payload: infer TPayload,
) => unknown
  ? TPayload
  : never

type EventResult<TEvents, TName extends keyof TEvents> = TEvents[TName] extends (
  payload: never,
) => infer TResult
  ? Awaited<TResult>
  : never

/** Minimal typed async emitter used for feature-to-feature daemon integration. */
export type SessionEventEmitter = {
  on<const TName extends keyof SessionEvents>(
    eventName: TName,
    listener: SessionEvents[TName],
  ): () => void
  emit<const TName extends keyof SessionEvents>(
    eventName: TName,
    payload: EventPayload<SessionEvents, TName>,
  ): Promise<Array<EventResult<SessionEvents, TName>>>
}

type SessionAttentionEvent = {
  sessionId: DaemonSessionId
  scope: InboxScope
  headline: InboxHeadline
  turnId: string | null
}

/** Events that represent session lifecycle changes other features may react to. */
export type SessionEvents = {
  "lifecycle.blocked": EventListener<
    SessionAttentionEvent & {
      reason: string
    }
  >
  "lifecycle.turnEnded": EventListener<SessionAttentionEvent>
  "lifecycle.replied": EventListener<{
    sessionId: DaemonSessionId
  }>
  "lifecycle.completed": EventListener<
    {
      sessionId: DaemonSessionId
    },
    InboxItem | null
  >
}

/** Creates the session feature event emitter provided to consuming daemon plugins. */
export function createSessionEventEmitter(): SessionEventEmitter {
  const listeners = new Map<keyof SessionEvents, Set<SessionEvents[keyof SessionEvents]>>()

  return {
    on(eventName, listener) {
      let eventListeners = listeners.get(eventName)
      if (!eventListeners) {
        eventListeners = new Set()
        listeners.set(eventName, eventListeners)
      }

      eventListeners.add(listener)
      return () => {
        eventListeners.delete(listener)
      }
    },
    async emit(eventName, payload) {
      const eventListeners = [...(listeners.get(eventName) ?? [])]
      const results = []

      for (const listener of eventListeners) {
        results.push(await listener(payload as never))
      }

      return results as Array<EventResult<SessionEvents, typeof eventName>>
    },
  }
}
