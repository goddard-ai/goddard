import type { DaemonLogger } from "@goddard-ai/daemon-plugin"
import { getErrorMessage } from "radashi"

import type { DaemonSession } from "../schema.ts"
import type { ActiveSession, SessionMemory } from "./session-memory.ts"

type SessionId = DaemonSession["id"]

/** Owns subscriber-count and timer decisions for loadable idle sessions. */
export function createIdleShutdownController(input: {
  memory: SessionMemory
  logger: DaemonLogger
  emitDiagnostic: (
    sessionId: SessionId,
    type: string,
    detail?: Record<string, unknown>,
    diagnosticLogger?: DaemonLogger,
  ) => void
  shutdownSession: (id: SessionId) => Promise<boolean>
}) {
  /** Returns how many `session.streamMessages` stream subscribers are attached to one session id. */
  function getSessionSubscriberCount(id: SessionId): number {
    return input.memory.sessionSubscriberCounts.get(id) ?? 0
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
    input.emitDiagnostic(
      active.id,
      "session_idle_shutdown_timer_cancelled",
      { reason, timeoutMs: active.idleShutdownTimeoutMs },
      active.logger,
    )
  }

  /** Re-checks whether one active session should have an idle auto-shutdown timer armed right now. */
  function refreshIdleShutdownState(id: SessionId, reason: string) {
    const active = input.memory.activeSessions.get(id)
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

    input.emitDiagnostic(
      active.id,
      "session_idle_shutdown_timer_started",
      { reason, timeoutMs: active.idleShutdownTimeoutMs },
      active.logger,
    )
    active.idleShutdownTimer = setTimeout(() => {
      void handleIdleShutdownTimerExpired(active.id).catch((error) => {
        input.logger.log("session_idle_shutdown_timer_failed", {
          sessionId: active.id,
          errorMessage: getErrorMessage(error),
        })
      })
    }, active.idleShutdownTimeoutMs)
  }

  /** Shuts down one loadable idle session when its auto-shutdown timer expires without any reconnect. */
  async function handleIdleShutdownTimerExpired(id: SessionId): Promise<void> {
    const active = input.memory.activeSessions.get(id)
    if (!active) {
      return
    }

    active.idleShutdownTimer = null
    if (!shouldStartIdleShutdownTimer(active)) {
      return
    }

    input.emitDiagnostic(
      id,
      "session_idle_shutdown_timer_expired",
      { timeoutMs: active.idleShutdownTimeoutMs },
      active.logger,
    )
    await input.shutdownSession(id)
  }

  /** Records one new `session.streamMessages` subscriber so idle shutdown waits for attached clients. */
  function sessionSubscriberConnected(id: SessionId): void {
    input.memory.sessionSubscriberCounts.set(id, getSessionSubscriberCount(id) + 1)
    refreshIdleShutdownState(id, "subscriber_connected")
  }

  /** Records one departing `session.streamMessages` subscriber and starts the timer when none remain. */
  function sessionSubscriberDisconnected(id: SessionId): void {
    const current = getSessionSubscriberCount(id)
    if (current <= 1) {
      input.memory.sessionSubscriberCounts.delete(id)
    } else {
      input.memory.sessionSubscriberCounts.set(id, current - 1)
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
