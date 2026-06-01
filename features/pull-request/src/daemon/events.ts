import type { AttentionHeadline, AttentionScope } from "@goddard-ai/schema/attention"

import type { PullRequestId } from "../schema.ts"

type MaybePromise<T> = T | Promise<T>

type EventListener<TPayload, TResult = void> = (payload: TPayload) => MaybePromise<TResult>

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
    payload: PullRequestAttentionEvent,
  ): Promise<void>
}

/** Events that represent pull-request lifecycle changes other features may react to. */
export type PullRequestEvents = {
  "lifecycle.created": EventListener<PullRequestAttentionEvent>
  "lifecycle.updated": EventListener<PullRequestAttentionEvent>
}

/** Creates the pull-request feature event emitter provided to consuming daemon plugins. */
export function createPullRequestEventEmitter(): PullRequestEventEmitter {
  const listeners: {
    [TName in keyof PullRequestEvents]: Set<PullRequestEvents[TName]>
  } = {
    "lifecycle.created": new Set(),
    "lifecycle.updated": new Set(),
  }

  return {
    on(eventName, listener) {
      listeners[eventName].add(listener)
      return () => {
        listeners[eventName].delete(listener)
      }
    },
    async emit(eventName, payload) {
      const eventListeners = [...listeners[eventName]]

      for (const listener of eventListeners) {
        await listener(payload)
      }
    },
  }
}
