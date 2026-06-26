import type { DaemonLogger } from "@goddard-ai/daemon-plugin"
import { getErrorMessage } from "radashi"

import type { SessionIdleShutdownUpdatedEvent } from "../events.ts"
import type { DaemonSession } from "../schema.ts"
import type { ActiveSession, SessionMemory } from "./session-memory.ts"

type SessionId = DaemonSession["id"]

/** Owns subscriber-count and timer decisions for loadable idle sessions. */
export function createIdleShutdownController({
  memory,
  logger,
  debug,
  emitDiagnostic,
  emitEvent,
  shutdownSession,
}: {
  memory: SessionMemory
  logger: DaemonLogger
  debug: (event: string, fields?: Record<string, unknown>) => void
  emitDiagnostic: (sessionId: SessionId, type: string, detail?: Record<string, unknown>) => void
  emitEvent: (event: SessionIdleShutdownUpdatedEvent) => void | Promise<void>
  shutdownSession: (id: SessionId) => Promise<boolean>
}) {
  function emitIdleShutdownUpdatedEvent(event: SessionIdleShutdownUpdatedEvent) {
    void emitEvent(event)
  }

  /** Returns how many `session.message event stream` stream subscribers are attached to one session id. */
  function getSessionSubscriberCount(id: SessionId): number {
    return memory.sessionSubscriberCounts.get(id) ?? 0
  }

  /** Checks whether one live session is quiescent enough for idle auto-shutdown. */
  function shouldStartIdleShutdownTimer(active: ActiveSession): boolean {
    return (
      active.supportsLoadSession &&
      getSessionSubscriberCount(active.id) === 0 &&
      active.activeTurn === null &&
      active.blockingPromptRequestId === null &&
      active.promptQueue.length === 0 &&
      active.pendingSteer === null &&
      active.lastPermissionRequest === null
    )
  }

  /** Cancels one pending idle auto-shutdown timer and records the reason for that cancellation. */
  function cancelIdleShutdownTimer(active: ActiveSession, reason: string) {
    if (!active.idleShutdownTimer) {
      return
    }

    clearTimeout(active.idleShutdownTimer)
    active.idleShutdownTimer = null
    debug("timer_cancelled", {
      sessionId: active.id,
      reason,
      timeoutMs: active.idleShutdownTimeoutMs,
    })
    emitIdleShutdownUpdatedEvent({
      sessionId: active.id,
      action: "cancelled",
      reason,
      timeoutMs: active.idleShutdownTimeoutMs,
    })
  }

  /** Re-checks whether one active session should have an idle auto-shutdown timer armed right now. */
  function refreshIdleShutdownState(id: SessionId, reason: string) {
    const active = memory.activeSessions.get(id)
    if (!active) {
      return
    }

    if (!shouldStartIdleShutdownTimer(active)) {
      cancelIdleShutdownTimer(active, reason)
      return
    }

    if (active.idleShutdownTimer) {
      return
    }

    debug("timer_started", {
      sessionId: active.id,
      reason,
      timeoutMs: active.idleShutdownTimeoutMs,
    })
    emitIdleShutdownUpdatedEvent({
      sessionId: active.id,
      action: "started",
      reason,
      timeoutMs: active.idleShutdownTimeoutMs,
    })
    active.idleShutdownTimer = setTimeout(() => {
      void handleIdleShutdownTimerExpired(active.id).catch((error) => {
        logger.log("session_idle_shutdown_timer_failed", {
          sessionId: active.id,
          errorMessage: getErrorMessage(error),
        })
      })
    }, active.idleShutdownTimeoutMs)
  }

  /** Shuts down one loadable idle session when its auto-shutdown timer expires without any reconnect. */
  async function handleIdleShutdownTimerExpired(id: SessionId): Promise<void> {
    const active = memory.activeSessions.get(id)
    if (!active) {
      return
    }

    active.idleShutdownTimer = null
    if (!shouldStartIdleShutdownTimer(active)) {
      return
    }

    emitDiagnostic(id, "session_idle_shutdown_timer_expired", {
      timeoutMs: active.idleShutdownTimeoutMs,
    })
    emitIdleShutdownUpdatedEvent({
      sessionId: id,
      action: "expired",
      timeoutMs: active.idleShutdownTimeoutMs,
    })
    await shutdownSession(id)
  }

  /** Records one new `session.message event stream` subscriber so idle shutdown waits for attached clients. */
  function sessionSubscriberConnected(id: SessionId): void {
    memory.sessionSubscriberCounts.set(id, getSessionSubscriberCount(id) + 1)
    refreshIdleShutdownState(id, "subscriber_connected")
  }

  /** Records one departing `session.message event stream` subscriber and starts the timer when none remain. */
  function sessionSubscriberDisconnected(id: SessionId): void {
    const current = getSessionSubscriberCount(id)
    if (current <= 1) {
      memory.sessionSubscriberCounts.delete(id)
    } else {
      memory.sessionSubscriberCounts.set(id, current - 1)
    }
    refreshIdleShutdownState(id, "subscriber_disconnected")
  }

  return {
    cancelIdleShutdownTimer,
    refreshIdleShutdownState,
    sessionSubscriberConnected,
    sessionSubscriberDisconnected,
  }
}
