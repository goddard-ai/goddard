import type * as acp from "@agentclientprotocol/sdk"
import { db } from "./store.ts"

/** Durable connectivity summary for a daemon session across daemon restarts. */
export type SessionConnectionMode = "live" | "history" | "none"

/** Structured session diagnostic event persisted for postmortem inspection. */
export type SessionDiagnosticEvent = {
  type: string
  at: string
  sessionId: string
  detail?: Record<string, unknown>
}

/** Durable daemon-owned session state that supplements the SQL session row. */
export type SessionStateRecord = {
  sessionId: string
  acpId: string
  connectionMode: SessionConnectionMode
  history: acp.AnyMessage[]
  diagnostics: SessionDiagnosticEvent[]
  activeDaemonSession: boolean
  createdAt: number
  updatedAt: number
}

/** Stored session-state payload including the internal kindstore document id. */
export type StoredSessionStateRecord = NonNullable<ReturnType<typeof db.sessionStates.get>>

/** Builds one new session-state input and lets kindstore manage ids and timestamps. */
export function createSessionStateRecord(
  record: Omit<SessionStateRecord, "createdAt" | "updatedAt">,
): Parameters<typeof db.sessionStates.create>[0] {
  return record
}

/** Applies a partial update to one persisted session-state record. */
export function applySessionStateUpdate(
  record: StoredSessionStateRecord,
  update: Partial<Omit<SessionStateRecord, "sessionId" | "createdAt" | "updatedAt">>,
): Parameters<typeof db.sessionStates.update>[1] extends infer TUpdater
  ? TUpdater extends (current: StoredSessionStateRecord) => infer TResult
    ? TResult
    : never
  : never {
  const { id: _id, createdAt: _createdAt, updatedAt: _updatedAt, ...rest } = record
  return {
    ...rest,
    ...update,
    sessionId: record.sessionId,
  }
}
