import type * as acp from "@agentclientprotocol/sdk"
import type { DaemonSessionMetadata } from "@goddard-ai/schema/daemon"
import type { SessionStatus } from "@goddard-ai/schema/db"
import { db } from "./store.ts"

/** Full durable daemon-session row accepted when creating a session record. */
export type SQLSessionInsert = {
  id: string
  acpId: string
  status: SessionStatus
  agentName: string
  cwd: string
  mcpServers: acp.McpServer[]
  errorMessage?: string | null
  blockedReason?: string | null
  initiative?: string | null
  lastAgentMessage?: string | null
  repository?: string | null
  prNumber?: number | null
  metadata?: DaemonSessionMetadata | null
  models?: acp.SessionModelState | null
}

/** Partial daemon-session updates that may mutate an existing durable session record. */
export type SQLSessionUpdate = Partial<SQLSessionInsert>

/** Stable cursor key used for recency-ordered daemon session pagination. */
export type SQLSessionListCursor = {
  updatedAt: Date
  id: string
}

/** Durable daemon-session row returned from kindstore-backed persistence. */
export type SQLSessionRecord = Required<
  Pick<SQLSessionInsert, "id" | "acpId" | "status" | "agentName" | "cwd" | "mcpServers">
> & {
  createdAt: Date
  updatedAt: Date
  errorMessage: string | null
  blockedReason: string | null
  initiative: string | null
  lastAgentMessage: string | null
  repository: string | null
  prNumber: number | null
  metadata: DaemonSessionMetadata | null
  models: acp.SessionModelState | null
}

export type StoredSessionRecord = NonNullable<ReturnType<typeof db.sessions.get>>

function toRepositoryPrKey(repository: string | null, prNumber: number | null) {
  return repository && typeof prNumber === "number" ? `${repository}#${prNumber}` : null
}

export function toSessionSortKey(updatedAt: number, sessionId: string) {
  return `${String(updatedAt).padStart(13, "0")}:${sessionId}`
}

/** Converts one session insert shape into the stored kindstore payload. */
export function normalizeSessionInsert(data: SQLSessionInsert) {
  const updatedAt = Date.now()
  const repository = data.repository ?? null
  const prNumber = typeof data.prNumber === "number" ? data.prNumber : null

  return {
    sessionId: data.id,
    acpId: data.acpId,
    status: data.status,
    agentName: data.agentName,
    cwd: data.cwd,
    mcpServers: data.mcpServers,
    errorMessage: data.errorMessage ?? null,
    blockedReason: data.blockedReason ?? null,
    initiative: data.initiative ?? null,
    lastAgentMessage: data.lastAgentMessage ?? null,
    repository,
    prNumber,
    repositoryPrKey: toRepositoryPrKey(repository, prNumber),
    metadata: data.metadata ?? null,
    models: data.models ?? null,
    sortKey: toSessionSortKey(updatedAt, data.id),
  }
}

/** Converts one stored kindstore session payload into the daemon-facing session record shape. */
export function fromStoredSession(value: StoredSessionRecord): SQLSessionRecord {
  return {
    id: value.sessionId,
    acpId: value.acpId,
    status: value.status,
    agentName: value.agentName,
    cwd: value.cwd,
    mcpServers: value.mcpServers,
    createdAt: new Date(value.createdAt),
    updatedAt: new Date(value.updatedAt),
    errorMessage: value.errorMessage,
    blockedReason: value.blockedReason,
    initiative: value.initiative,
    lastAgentMessage: value.lastAgentMessage,
    repository: value.repository,
    prNumber: value.prNumber,
    metadata: value.metadata,
    models: value.models,
  }
}

/** Applies one partial session update to the stored kindstore payload. */
export function applySessionUpdate(current: StoredSessionRecord, data: SQLSessionUpdate) {
  const nextUpdatedAt = Date.now()
  const repository = data.repository === undefined ? current.repository : (data.repository ?? null)
  const prNumber =
    data.prNumber === undefined
      ? current.prNumber
      : typeof data.prNumber === "number"
        ? data.prNumber
        : null

  const { id: _id, createdAt: _createdAt, updatedAt: _updatedAt, ...rest } = current

  return {
    ...rest,
    ...(data.acpId === undefined ? {} : { acpId: data.acpId }),
    ...(data.status === undefined ? {} : { status: data.status }),
    ...(data.agentName === undefined ? {} : { agentName: data.agentName }),
    ...(data.cwd === undefined ? {} : { cwd: data.cwd }),
    ...(data.mcpServers === undefined ? {} : { mcpServers: data.mcpServers }),
    ...(data.errorMessage === undefined ? {} : { errorMessage: data.errorMessage ?? null }),
    ...(data.blockedReason === undefined ? {} : { blockedReason: data.blockedReason ?? null }),
    ...(data.initiative === undefined ? {} : { initiative: data.initiative ?? null }),
    ...(data.lastAgentMessage === undefined
      ? {}
      : { lastAgentMessage: data.lastAgentMessage ?? null }),
    ...(data.metadata === undefined ? {} : { metadata: data.metadata ?? null }),
    ...(data.models === undefined ? {} : { models: data.models ?? null }),
    repository,
    prNumber,
    repositoryPrKey: toRepositoryPrKey(repository, prNumber),
    sortKey: toSessionSortKey(nextUpdatedAt, current.sessionId),
  }
}
