/** Typed lifecycle events emitted by the session feature for downstream daemon plugins. */
import type { InboxHeadline, InboxScope } from "@goddard-ai/inbox/schema"
import type { CreateSessionRequest } from "@goddard-ai/schema/daemon"
import type { DaemonSessionId } from "@goddard-ai/schema/id"
import mitt, { type Handler } from "mitt"

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

type SessionEventPayloads = {
  [TName in keyof SessionEvents]: EventPayload<SessionEvents, TName>
}

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

/** Worktree lifecycle data exposed to daemon plugins without exposing session persistence. */
export type SessionWorktreeLifecycleState = {
  sessionId: DaemonSessionId
  repoRoot: string
  requestedCwd: string
  effectiveCwd: string
  worktreeDir: string
  branchName: string
  poweredBy: string
}

/** Worktree lifecycle event payload shared by launch-time session hooks. */
type SessionWorktreeLifecycleEvent = {
  sessionId: DaemonSessionId
  worktree: SessionWorktreeLifecycleState
}

/** Events that represent session lifecycle changes other features may react to. */
export type SessionEvents = {
  "lifecycle.worktreePrepared": EventListener<
    SessionWorktreeLifecycleEvent & {
      request: CreateSessionRequest
    }
  >
  "lifecycle.sessionPersisted": EventListener<{
    sessionId: DaemonSessionId
    request: CreateSessionRequest
  }>
  "lifecycle.sessionActivated": EventListener<{
    sessionId: DaemonSessionId
    worktree: SessionWorktreeLifecycleState | null
  }>
  "lifecycle.launchFinished": EventListener<
    SessionWorktreeLifecycleEvent & {
      reason: "one_shot_completed"
    }
  >
  "lifecycle.launchFailed": EventListener<
    SessionWorktreeLifecycleEvent & {
      error: unknown
    }
  >
  "lifecycle.sessionStopping": EventListener<{
    sessionId: DaemonSessionId
    reason: "agent_process_exit" | "session_shutdown" | "daemon_shutdown"
    worktree: SessionWorktreeLifecycleState | null
  }>
  "lifecycle.blocked": EventListener<
    SessionAttentionEvent & {
      reason: string
    }
  >
  "lifecycle.turnEnded": EventListener<SessionAttentionEvent>
  "lifecycle.replied": EventListener<{
    sessionId: DaemonSessionId
  }>
  "lifecycle.completed": EventListener<{
    sessionId: DaemonSessionId
  }>
}

/** Creates the session feature event emitter provided to consuming daemon plugins. */
export function createSessionEventEmitter(): SessionEventEmitter {
  const emitter = mitt<SessionEventPayloads>()

  return {
    on(eventName, listener) {
      const handler = listener as Handler<SessionEventPayloads[typeof eventName]>
      emitter.on(eventName, handler)
      return () => {
        emitter.off(eventName, handler)
      }
    },
    async emit(eventName, payload) {
      const eventListeners = [...(emitter.all.get(eventName) ?? [])] as Array<
        SessionEvents[typeof eventName]
      >
      const results = []

      for (const listener of eventListeners) {
        results.push(await listener(payload as never))
      }

      return results as Array<EventResult<SessionEvents, typeof eventName>>
    },
  }
}
