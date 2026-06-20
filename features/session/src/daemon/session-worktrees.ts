import { IpcClientError } from "@goddard-ai/ipc"

import type { SessionDb } from "../daemon.ts"
import type { DaemonSession, DaemonWorktree, GetSessionWorktreeResponse } from "../schema.ts"
import type { SessionEventEmitter } from "./manager.ts"
import type { SessionMemory } from "./session-memory.ts"
import { inspectWorktreeCompletionState, type SessionWorktreeState } from "./worktree.ts"

type SessionId = DaemonSession["id"]
type SessionWorktreeDoc = DaemonWorktree

/** Worktree lifecycle data exposed to daemon plugins without exposing session persistence. */
export type SessionWorktreeLifecycleState = {
  sessionId: SessionId
  repoRoot: string
  requestedCwd: string
  effectiveCwd: string
  worktreeDir: string
  branchName: string
  poweredBy: string
}

export function toSessionWorktreeValue(record: SessionWorktreeDoc) {
  const { id: _id, sessionId: _sessionId, ...worktree } = record
  return worktree
}

export function toSessionWorktreeLifecycleState(
  record: SessionWorktreeDoc | SessionWorktreeState,
  sessionId: SessionId,
): SessionWorktreeLifecycleState {
  return {
    sessionId,
    repoRoot: record.repoRoot,
    requestedCwd: record.requestedCwd,
    effectiveCwd: record.effectiveCwd,
    worktreeDir: record.worktreeDir,
    branchName: record.branchName,
    poweredBy: record.poweredBy,
  }
}

/** Owns session worktree lookup, lifecycle projection, and completion safety checks. */
export function createSessionWorktreeFeature({
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

  async function resolvePersistedWorktreeRecord(id: SessionId) {
    return (
      db.worktrees.first({
        where: { sessionId: id },
      }) ?? null
    )
  }

  async function getWorktree(id: SessionId): Promise<GetSessionWorktreeResponse> {
    const session = requireSessionDocument(id)
    const worktreeRecord = await resolvePersistedWorktreeRecord(id)

    return {
      id: session.id,
      acpSessionId: session.acpSessionId,
      worktree: worktreeRecord ? toSessionWorktreeValue(worktreeRecord) : null,
    }
  }

  async function requireWorktree(id: SessionId): Promise<SessionWorktreeLifecycleState> {
    const worktreeRecord = await resolvePersistedWorktreeRecord(id)
    if (!worktreeRecord) {
      throw new IpcClientError(`Session ${id} does not have a daemon worktree`)
    }

    return toSessionWorktreeLifecycleState(worktreeRecord, id)
  }

  async function listWorktrees(): Promise<SessionWorktreeLifecycleState[]> {
    return db.worktrees
      .findMany()
      .map((record: DaemonWorktree) => toSessionWorktreeLifecycleState(record, record.sessionId))
  }

  async function findWorktreeByDir(worktreeDir: string) {
    const record =
      db.worktrees
        .findMany()
        .find((worktreeRecord: DaemonWorktree) => worktreeRecord.worktreeDir === worktreeDir) ??
      null
    return record ? toSessionWorktreeLifecycleState(record, record.sessionId) : null
  }

  async function completeSession(id: SessionId) {
    requireSessionDocument(id)
    const active = memory.activeSessions.get(id) ?? null
    if (active?.activeTurn) {
      throw new IpcClientError("Cannot complete a session while the agent has an active turn")
    }

    const worktreeRecord = await resolvePersistedWorktreeRecord(id)
    if (worktreeRecord) {
      let completionState: Awaited<ReturnType<typeof inspectWorktreeCompletionState>>
      try {
        completionState = await inspectWorktreeCompletionState(worktreeRecord)
      } catch {
        throw new IpcClientError(
          "Cannot complete a worktree session because its git state could not be inspected",
        )
      }

      if (completionState.dirty) {
        throw new IpcClientError(
          "Cannot complete a worktree session while its working tree has uncommitted changes",
        )
      }

      if (completionState.unmergedCommits) {
        throw new IpcClientError(
          "Cannot complete a worktree session while it has commits that have not been merged into the primary checkout",
        )
      }
    }

    updateSessionActivity(id, {
      completedHidden: true,
    })
    await events.emit("session.completed", {
      sessionId: id,
    })
    return requireSessionDocument(id)
  }

  return {
    completeSession,
    findWorktreeByDir,
    getWorktree,
    listWorktrees,
    requireWorktree,
    resolvePersistedWorktreeRecord,
  }
}
