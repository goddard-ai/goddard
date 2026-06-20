import type { DaemonLogger } from "@goddard-ai/daemon-plugin"
import { getAcpMessageResult } from "acp-client"
import type * as acp from "acp-client/protocol"

import type { SessionDb } from "../daemon.ts"
import type {
  DaemonSessionTurnDraft,
  SessionHistoryTurn,
  SessionLifecycleField,
} from "../schema.ts"
import type { ActiveSession } from "./session-memory.ts"
import {
  appendSessionHistoryMessage,
  getAvailableCommandsFromMessage,
  isTurnTerminalMessage,
  shouldFlushTurnDraftImmediately,
  toCompletedTurnInput,
  toSessionHistoryTurnFromDraft,
  toTurnDraftInput,
  type ActiveTurnBuffer,
} from "./turn-history.ts"

type SessionTurnDraftDoc = DaemonSessionTurnDraft

/** Owns active-turn draft persistence and completed-turn finalization for live sessions. */
export function createActiveTurnStore(input: {
  db: SessionDb
  debug: (event: string, fields?: Record<string, unknown>) => void
  emitDiagnostic: (
    sessionId: ActiveSession["id"],
    type: string,
    detail?: Record<string, unknown>,
    diagnosticLogger?: DaemonLogger,
  ) => void
  publishSessionUpdated: (
    id: ActiveSession["id"],
    changed: readonly SessionLifecycleField[],
  ) => void
  refreshIdleShutdownState: (id: ActiveSession["id"], reason: string) => void
  updateSessionAvailableCommands: (
    sessionId: ActiveSession["id"],
    availableCommands: acp.AvailableCommand[],
  ) => void
  updateSessionContextUsage: (sessionId: ActiveSession["id"], message: acp.AnyMessage) => boolean
}) {
  const db = input.db

  function clearTurnDraftFlushTimer(activeTurn: ActiveTurnBuffer | null) {
    if (!activeTurn?.flushTimer) {
      return
    }

    clearTimeout(activeTurn.flushTimer)
    activeTurn.flushTimer = null
  }

  function flushActiveTurnDraft(active: ActiveSession, reason: string) {
    const activeTurn = active.activeTurn
    if (!activeTurn) {
      input.debug("session.turns.flush_skipped", {
        sessionId: active.id,
        reason,
        skippedReason: "no_active_turn",
      })
      return
    }

    clearTurnDraftFlushTimer(activeTurn)
    const existingDraft =
      (activeTurn.draftId && db.sessionTurnDrafts.get(activeTurn.draftId)) ||
      db.sessionTurnDrafts.first({
        where: { sessionId: active.id },
      }) ||
      null
    const draftInput = toTurnDraftInput(active.id, activeTurn)

    if (existingDraft) {
      activeTurn.draftId = existingDraft.id
      db.sessionTurnDrafts.put(existingDraft.id, draftInput)
    } else {
      activeTurn.draftId = db.sessionTurnDrafts.create(draftInput).id
    }
    input.debug("session.turns.draft_flushed", {
      sessionId: active.id,
      reason,
      turnId: activeTurn.turnId,
      sequence: activeTurn.sequence,
      draftId: activeTurn.draftId,
      messageCount: activeTurn.messages.length,
      updatedExistingDraft: Boolean(existingDraft),
    })

    input.emitDiagnostic(
      active.id,
      "session_turn_draft_flushed",
      {
        reason,
        turnId: activeTurn.turnId,
        sequence: activeTurn.sequence,
        messageCount: activeTurn.messages.length,
      },
      active.logger,
    )
  }

  function scheduleActiveTurnDraftFlush(active: ActiveSession, reason: string, immediate = false) {
    const activeTurn = active.activeTurn
    if (!activeTurn) {
      input.debug("session.turns.flush_schedule_skipped", {
        sessionId: active.id,
        reason,
        skippedReason: "no_active_turn",
      })
      return
    }

    if (immediate) {
      input.debug("session.turns.flush_immediate", {
        sessionId: active.id,
        reason,
        turnId: activeTurn.turnId,
        sequence: activeTurn.sequence,
      })
      flushActiveTurnDraft(active, reason)
      return
    }

    clearTurnDraftFlushTimer(activeTurn)
    input.debug("session.turns.flush_scheduled", {
      sessionId: active.id,
      reason,
      turnId: activeTurn.turnId,
      sequence: activeTurn.sequence,
      messageCount: activeTurn.messages.length,
    })
    activeTurn.flushTimer = setTimeout(() => {
      try {
        flushActiveTurnDraft(active, reason)
      } catch {}
    }, 100)
  }

  function persistTurnDraftAsInterruptedTurn(
    sessionId: ActiveSession["id"],
    draftRecord: SessionTurnDraftDoc,
    diagnosticLogger: DaemonLogger,
  ) {
    const existingTurn =
      db.sessionTurns.first({
        where: { sessionId, sequence: draftRecord.sequence },
      }) ?? null

    if (existingTurn?.turnId === draftRecord.turnId) {
      db.sessionTurnDrafts.delete(draftRecord.id)
      input.debug("session.turns.draft_promotion_skipped", {
        sessionId,
        draftId: draftRecord.id,
        turnId: draftRecord.turnId,
        sequence: draftRecord.sequence,
        reason: "matching_turn_exists",
      })
      return existingTurn
    }

    const turn = toSessionHistoryTurnFromDraft(draftRecord)
    const createdTurn = existingTurn
      ? db.sessionTurns.put(existingTurn.id, toCompletedTurnInput(sessionId, turn))
      : db.sessionTurns.create(toCompletedTurnInput(sessionId, turn))

    db.sessionTurnDrafts.delete(draftRecord.id)
    input.debug("session.turns.draft_promoted", {
      sessionId,
      draftId: draftRecord.id,
      turnId: draftRecord.turnId,
      sequence: draftRecord.sequence,
      replacedExistingTurn: Boolean(existingTurn),
      messageCount: draftRecord.messages.length,
    })
    input.emitDiagnostic(
      sessionId,
      "session_turn_draft_promoted",
      {
        turnId: draftRecord.turnId,
        sequence: draftRecord.sequence,
      },
      diagnosticLogger,
    )
    return createdTurn
  }

  function appendTurnScopedMessage(active: ActiveSession, message: acp.AnyMessage) {
    const availableCommands = getAvailableCommandsFromMessage(message)
    if (availableCommands) {
      input.debug("session.turns.available_commands_updated", {
        sessionId: active.id,
        commandCount: availableCommands.length,
      })
      input.updateSessionAvailableCommands(active.id, availableCommands)
    }

    if (input.updateSessionContextUsage(active.id, message)) {
      input.debug("session.turns.message_skipped", {
        sessionId: active.id,
        reason: "context_usage_update",
      })
      return
    }

    const activeTurn = active.activeTurn
    if (!activeTurn) {
      input.debug("session.turns.message_skipped", {
        sessionId: active.id,
        reason: "no_active_turn",
      })
      return
    }

    const turnMessage = appendSessionHistoryMessage(activeTurn.messages, message)
    if (!turnMessage) {
      input.debug("session.turns.message_skipped", {
        sessionId: active.id,
        turnId: activeTurn.turnId,
        sequence: activeTurn.sequence,
        reason: "history_message_not_persistable",
        method: "method" in message ? message.method : undefined,
        hasId: "id" in message && message.id != null,
      })
      return null
    }
    input.debug("session.turns.message_appended", {
      sessionId: active.id,
      turnId: activeTurn.turnId,
      sequence: activeTurn.sequence,
      messageSequence: turnMessage.sequence,
      messageCount: activeTurn.messages.length,
      method: "method" in message ? message.method : undefined,
      hasId: "id" in message && message.id != null,
    })
    scheduleActiveTurnDraftFlush(
      active,
      shouldFlushTurnDraftImmediately(activeTurn, message) ? "boundary" : "stream",
      shouldFlushTurnDraftImmediately(activeTurn, message),
    )
    return turnMessage
  }

  function finalizeActiveTurn(active: ActiveSession, message: acp.AnyMessage) {
    const activeTurn = active.activeTurn
    if (!activeTurn || !isTurnTerminalMessage(activeTurn, message)) {
      input.debug("session.turns.finalize_skipped", {
        sessionId: active.id,
        reason: activeTurn ? "non_terminal_message" : "no_active_turn",
        method: "method" in message ? message.method : undefined,
        hasId: "id" in message && message.id != null,
      })
      return
    }

    const completionKind = "error" in message ? "error" : "result"
    const stopReason =
      completionKind === "result"
        ? (getAcpMessageResult<acp.PromptResponse>(message)?.stopReason ?? null)
        : null
    const completedTurn: SessionHistoryTurn = {
      turnId: activeTurn.turnId,
      sequence: activeTurn.sequence,
      promptRequestId: activeTurn.promptRequestId,
      startedAt: activeTurn.startedAt,
      completedAt: new Date().toISOString(),
      completionKind,
      stopReason,
      inboxScope: activeTurn.inboxScope ?? null,
      inboxHeadline: activeTurn.inboxHeadline ?? null,
      messages: [...activeTurn.messages],
    }

    flushActiveTurnDraft(active, "completion")
    db.batch(() => {
      db.sessionTurns.create(toCompletedTurnInput(active.id, completedTurn))
      if (activeTurn.draftId) {
        db.sessionTurnDrafts.delete(activeTurn.draftId)
      } else {
        const draftRecord =
          db.sessionTurnDrafts.first({
            where: { sessionId: active.id },
          }) ?? null
        if (draftRecord) {
          db.sessionTurnDrafts.delete(draftRecord.id)
        }
      }
    })
    clearTurnDraftFlushTimer(activeTurn)
    active.activeTurn = null
    active.nextTurnSequence = Math.max(active.nextTurnSequence, completedTurn.sequence + 1)
    input.publishSessionUpdated(active.id, ["activeTurn"])
    input.refreshIdleShutdownState(active.id, "turn_completed")
    input.debug("session.turns.finalized", {
      sessionId: active.id,
      turnId: completedTurn.turnId,
      sequence: completedTurn.sequence,
      completionKind,
      stopReason: stopReason ?? undefined,
      messageCount: completedTurn.messages.length,
    })
    input.emitDiagnostic(
      active.id,
      "session_turn_persisted",
      {
        turnId: completedTurn.turnId,
        sequence: completedTurn.sequence,
        completionKind: completedTurn.completionKind,
        stopReason: completedTurn.stopReason ?? undefined,
        messageCount: completedTurn.messages.length,
      },
      active.logger,
    )
  }

  return {
    appendTurnScopedMessage,
    clearTurnDraftFlushTimer,
    finalizeActiveTurn,
    flushActiveTurnDraft,
    persistTurnDraftAsInterruptedTurn,
    scheduleActiveTurnDraftFlush,
  }
}
