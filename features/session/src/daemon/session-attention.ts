import { IpcClientError } from "@goddard-ai/ipc"
import type {
  AttentionHeadline,
  AttentionMetadataInput,
  AttentionScope,
} from "@goddard-ai/schema/attention"

import type { SessionDb } from "../daemon.ts"
import type { DaemonSession } from "../schema.ts"
import type { SessionEventEmitter } from "./manager.ts"
import { resolveSessionAttentionMetadata } from "./metadata.ts"
import type { SessionMemory } from "./session-memory.ts"

type SessionId = DaemonSession["id"]
type SessionDoc = DaemonSession

/** Owns user-visible attention state: initiatives, blockers, turn-ended metadata, and results. */
export function createSessionAttentionFeature({
  db,
  memory,
  events,
  updateSessionActivity,
}: {
  db: SessionDb
  memory: SessionMemory
  events: SessionEventEmitter
  updateSessionActivity: (id: SessionId, update: Partial<DaemonSession>) => void
}) {
  function requireSessionDocument(id: SessionId) {
    const record = db.sessions.get(id) ?? null
    if (!record) {
      throw new IpcClientError(`Unknown session: ${id}`)
    }

    return record
  }

  function resolveCurrentTurnId(id: SessionId) {
    const activeTurn = memory.activeSessions.get(id)?.activeTurn ?? null
    if (activeTurn) {
      return activeTurn.turnId
    }

    return (
      db.sessionTurns.first({
        where: { sessionId: id },
        orderBy: {
          sessionId: "asc",
          sequence: "desc",
        },
      })?.turnId ?? null
    )
  }

  function applyInboxMetadataToCurrentTurn(
    id: SessionId,
    metadata: { scope: AttentionScope; headline: AttentionHeadline },
  ) {
    const activeTurn = memory.activeSessions.get(id)?.activeTurn ?? null
    if (activeTurn) {
      activeTurn.inboxScope = metadata.scope
      activeTurn.inboxHeadline = metadata.headline
      return
    }

    const latestTurn =
      db.sessionTurns.first({
        where: { sessionId: id },
        orderBy: {
          sessionId: "asc",
          sequence: "desc",
        },
      }) ?? null
    if (latestTurn) {
      db.sessionTurns.update(latestTurn.id, {
        inboxScope: metadata.scope,
        inboxHeadline: metadata.headline,
      })
    }
  }

  function resolveAndPersistInboxMetadata(inputMetadata: {
    session: SessionDoc
    metadata?: AttentionMetadataInput & { fallbackHeadline?: string }
    blockedReason?: string | null
  }) {
    const resolved = resolveSessionAttentionMetadata({
      session: {
        ...inputMetadata.session,
        blockedReason: inputMetadata.blockedReason ?? inputMetadata.session.blockedReason,
      },
      scope: inputMetadata.metadata?.scope,
      headline: inputMetadata.metadata?.headline,
      fallbackHeadline: inputMetadata.metadata?.fallbackHeadline,
    })
    applyInboxMetadataToCurrentTurn(inputMetadata.session.id, resolved)
    return resolved
  }

  async function declareInitiative(id: SessionId, title: string) {
    requireSessionDocument(id)
    updateSessionActivity(id, {
      status: "active",
      completedHidden: false,
      initiative: title,
      blockedReason: null,
    })

    return requireSessionDocument(id)
  }

  async function reportBlocker(
    id: SessionId,
    reason: string,
    metadata: AttentionMetadataInput = {},
  ) {
    const session = requireSessionDocument(id)
    const resolved = resolveAndPersistInboxMetadata({
      session,
      metadata: {
        ...metadata,
        fallbackHeadline: reason,
      },
      blockedReason: reason,
    })
    updateSessionActivity(id, {
      status: "blocked",
      completedHidden: false,
      blockedReason: reason,
      inboxScope: resolved.scope,
    })
    await events.emit("session.blocked", {
      sessionId: id,
      reason,
      scope: resolved.scope,
      headline: resolved.headline,
      turnId: resolveCurrentTurnId(id),
    })

    return requireSessionDocument(id)
  }

  async function reportTurnEnded(id: SessionId, metadata: AttentionMetadataInput = {}) {
    const session = requireSessionDocument(id)
    const resolved = resolveAndPersistInboxMetadata({
      session,
      metadata: {
        ...metadata,
        fallbackHeadline: session.lastAgentMessage ?? session.initiative ?? session.title,
      },
    })
    updateSessionActivity(id, {
      status: "done",
      completedHidden: false,
      initiative: null,
      blockedReason: null,
      inboxScope: resolved.scope,
    })

    const activeTurn = memory.activeSessions.get(id)?.activeTurn ?? null
    if (activeTurn?.touchedAttentionEntity !== true) {
      await events.emit("session.turn.ended", {
        sessionId: id,
        scope: resolved.scope,
        headline: resolved.headline,
        turnId: resolveCurrentTurnId(id),
      })
    }

    return requireSessionDocument(id)
  }

  async function recordTurnAttentionActivity(
    id: SessionId,
    metadata: AttentionMetadataInput & { fallbackHeadline?: string } = {},
  ) {
    const session = requireSessionDocument(id)
    const resolved = resolveAndPersistInboxMetadata({
      session,
      metadata,
    })
    const activeTurn = memory.activeSessions.get(id)?.activeTurn ?? null
    if (activeTurn) {
      activeTurn.touchedAttentionEntity = true
    }
    updateSessionActivity(id, {
      inboxScope: resolved.scope,
    })

    return {
      scope: resolved.scope,
      headline: resolved.headline,
      turnId: resolveCurrentTurnId(id),
    }
  }

  async function recordSessionResult(id: SessionId, message: string) {
    requireSessionDocument(id)
    updateSessionActivity(id, {
      status: "done",
      lastAgentMessage: message,
    })
  }

  return {
    declareInitiative,
    reportBlocker,
    reportTurnEnded,
    recordTurnAttentionActivity,
    recordSessionResult,
  }
}
