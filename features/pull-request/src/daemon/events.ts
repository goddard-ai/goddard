import type { AttentionHeadline, AttentionScope } from "@goddard-ai/schema/attention"
import mitt, { type Handler } from "mitt"

import type { PullRequestId } from "../schema.ts"

type MaybePromise<T> = T | Promise<T>

type EventListener<TPayload, TResult = void> = (payload: TPayload) => MaybePromise<TResult>

type EventPayload<TEvents, TName extends keyof TEvents> = TEvents[TName] extends (
  payload: infer TPayload,
) => unknown
  ? TPayload
  : never

type PullRequestEventPayloads = {
  [TName in keyof PullRequestEvents]: EventPayload<PullRequestEvents, TName>
}

type PullRequestAttentionEvent = {
  pullRequestId: PullRequestId
  scope: AttentionScope
  headline: AttentionHeadline
  turnId: string | null
}

/** Minimal typed async emitter for pull-request lifecycle integrations. */
export type PullRequestEventEmitter = {
  on<const TName extends keyof PullRequestEvents>(
    eventName: TName,
    listener: PullRequestEvents[TName],
  ): () => void
  emit<const TName extends keyof PullRequestEvents>(
    eventName: TName,
    payload: EventPayload<PullRequestEvents, TName>,
  ): Promise<void>
}

/** Events that represent pull-request lifecycle changes other features may react to. */
export type PullRequestEvents = {
  "lifecycle.created": EventListener<PullRequestAttentionEvent>
  "lifecycle.updated": EventListener<PullRequestAttentionEvent>
}

/** Creates the pull-request feature event emitter provided to consuming daemon plugins. */
export function createPullRequestEventEmitter(): PullRequestEventEmitter {
  const emitter = mitt<PullRequestEventPayloads>()

  return {
    on(eventName, listener) {
      const handler = listener as Handler<PullRequestEventPayloads[typeof eventName]>
      emitter.on(eventName, handler)
      return () => {
        emitter.off(eventName, handler)
      }
    },
    async emit(eventName, payload) {
      const eventListeners = [...(emitter.all.get(eventName) ?? [])] as Array<
        PullRequestEvents[typeof eventName]
      >

      for (const listener of eventListeners) {
        await listener(payload as never)
      }
    },
  }
}
