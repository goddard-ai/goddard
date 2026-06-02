import { IpcClientError } from "@goddard-ai/ipc"
import type { AgentDistribution } from "@goddard-ai/schema/agent-distribution"
import type * as acp from "acp-client/protocol"

import type { SessionDb } from "../daemon.ts"
import type {
  CreateSessionRequest,
  DaemonSession,
  DaemonSessionMetadata,
  DaemonSessionTurnDraft,
  DaemonWorktree,
  SessionConnection,
  SessionHistoryTurn,
} from "../schema.ts"
import { toCompletedTurnInput } from "./turn-history.ts"
import type { PreparedSessionWorktree, SessionWorktreeState } from "./worktree.ts"

type SessionId = DaemonSession["id"]

/** Durable connectivity summary for a daemon session across daemon restarts. */
export type SessionConnectionMode = DaemonSession["connectionMode"]
type SessionDoc = DaemonSession
type SessionTurnDraftDoc = DaemonSessionTurnDraft
type SessionWorktreeDoc = DaemonWorktree

/** Loads persisted session-side artifacts that need to be reused during launch. */
export type ExistingSessionArtifacts = {
  draftRecord: SessionTurnDraftDoc | null
  nextTurnSequence: number
  worktreeRecord: SessionWorktreeDoc | null
  worktree: SessionWorktreeState | null
}

/** Internal launch request shape after the daemon has resolved the effective agent. */
export type ResolvedCreateSessionRequest = Omit<CreateSessionRequest, "agent" | "systemPrompt"> & {
  agent: NonNullable<CreateSessionRequest["agent"]>
  systemPrompt: string
}

/** Reads the highest persisted turn sequence across completed turns and any durable draft. */
export function resolveLatestStoredTurnSequence(db: SessionDb, id: SessionId) {
  const latestTurn =
    db.sessionTurns.first({
      where: { sessionId: id },
      orderBy: {
        sessionId: "asc",
        sequence: "desc",
      },
    }) ?? null
  const draft =
    db.sessionTurnDrafts.first({
      where: { sessionId: id },
      orderBy: {
        sessionId: "asc",
        sequence: "desc",
      },
    }) ?? null

  return Math.max(latestTurn?.sequence ?? 0, draft?.sequence ?? 0)
}

/** Loads any persisted session-side artifacts that need to be reused during launch. */
export function resolveExistingSessionArtifacts(
  db: SessionDb,
  id: SessionId,
  existingSession: SessionDoc | null,
): ExistingSessionArtifacts {
  if (!existingSession) {
    return {
      draftRecord: null,
      nextTurnSequence: 1,
      worktreeRecord: null,
      worktree: null,
    }
  }

  const draftRecord =
    db.sessionTurnDrafts.first({
      where: { sessionId: id },
    }) ?? null
  const worktreeRecord =
    db.worktrees.first({
      where: { sessionId: id },
    }) ?? null
  return {
    draftRecord,
    nextTurnSequence: resolveLatestStoredTurnSequence(db, id) + 1,
    worktreeRecord,
    worktree: toSessionWorktreeState(worktreeRecord),
  }
}

/** Removes kindstore-only identity fields from one persisted worktree record. */
function toSessionWorktreeState(record: SessionWorktreeDoc | null) {
  if (!record) {
    return null
  }

  const { id: _id, sessionId: _sessionId, ...worktree } = record
  return worktree
}

/** Derives reconnectability from stored connection state without joining adjacent kinds. */
export function toConnectionState(input: {
  mode: SessionConnectionMode
  activeDaemonSession: boolean
}): SessionConnection {
  return {
    mode: input.mode,
    reconnectable: input.mode === "live",
    activeDaemonSession: input.activeDaemonSession,
  }
}

/** Chooses the archived connection mode from whether any session turn history survived persistence. */
export function archivedConnectionMode(hasHistory: boolean): SessionConnectionMode {
  return hasHistory ? "history" : "none"
}

/** Keeps reloadable sessions live even when the current daemon process has already detached. */
export function disconnectedConnectionMode(
  hasHistory: boolean,
  supportsLoadSession: boolean,
): SessionConnectionMode {
  return supportsLoadSession ? "live" : archivedConnectionMode(hasHistory)
}

/** Merges structured session metadata layers while dropping empty results. */
export function mergeSessionMetadata(
  a: DaemonSessionMetadata | null | undefined,
  b: DaemonSessionMetadata | null | undefined,
): DaemonSessionMetadata | undefined {
  const merged = { ...a, ...b }
  return Object.keys(merged).length > 0 ? merged : undefined
}

/** Produces a stable agent name whether the request used an id or a resolved distribution. */
export function agentNameFromInput(agent: string | AgentDistribution): string {
  if (typeof agent === "string") {
    return agent
  }

  return agent.name
}

/** Rebuilds the daemon launch request used to reconnect one previously stored session. */
export function createReconnectRequest(session: SessionDoc): CreateSessionRequest {
  return {
    agent: session.agent ?? undefined,
    cwd: session.cwd,
    mcpServers: session.mcpServers,
    systemPrompt: "",
    repository: session.repository ?? undefined,
    prNumber: session.prNumber ?? undefined,
    metadata: session.metadata ?? undefined,
  }
}

/** Extracts repository ownership fields used for permission scoping and persistence. */
export function parseRepoScope(params: { repository?: string; prNumber?: number }): {
  repository: string | null
  prNumber: number | null
  owner: string
  repo: string
  allowedPrNumbers: number[]
} {
  const repository = params.repository?.trim() ?? ""
  const prNumber = typeof params.prNumber === "number" ? params.prNumber : null
  const [owner, repo] = repository.split("/")

  return {
    repository: repository.length > 0 ? repository : null,
    prNumber,
    owner: owner ?? "",
    repo: repo ?? "",
    allowedPrNumbers: prNumber === null ? [] : [prNumber],
  }
}

/** Builds the persisted daemon session record written after ACP session initialization completes. */
export function createSessionRecordUpdate(params: {
  initialized: {
    acpSessionId: string
    status: DaemonSession["status"]
    stopReason: acp.PromptResponse["stopReason"] | null
    models?: acp.SessionModelState | null
    configOptions?: acp.SessionConfigOption[] | null
  }
  request: ResolvedCreateSessionRequest
  cwd: string
  token: string
  scope: ReturnType<typeof parseRepoScope>
  nextPermission: {
    owner: string
    repo: string
    allowedPrNumbers: number[]
  }
  sessionMetadata: DaemonSessionMetadata | null | undefined
  existingSession: SessionDoc | null
  exitAfterInitialPrompt: boolean
  supportsLoadSession: boolean
  title: string
  titleState: DaemonSession["titleState"]
  availableCommands: acp.AvailableCommand[]
  contextUsage: DaemonSession["contextUsage"]
}) {
  const connectionMode: SessionConnectionMode =
    params.exitAfterInitialPrompt && !params.supportsLoadSession ? "history" : "live"

  return {
    acpSessionId: params.initialized.acpSessionId,
    status: params.initialized.status,
    stopReason: params.initialized.stopReason ?? params.existingSession?.stopReason ?? null,
    agent: params.request.agent,
    agentName: agentNameFromInput(params.request.agent),
    cwd: params.cwd,
    title: params.existingSession?.title ?? params.title,
    titleState: params.existingSession?.titleState ?? params.titleState,
    mcpServers: params.request.mcpServers,
    connectionMode,
    supportsLoadSession: params.supportsLoadSession,
    activeDaemonSession: !params.exitAfterInitialPrompt,
    completedHidden: false,
    repository: params.scope.repository,
    prNumber: params.scope.prNumber,
    token: params.token,
    permissions: params.nextPermission,
    metadata: params.sessionMetadata ?? null,
    models: params.initialized.models ?? params.existingSession?.models ?? null,
    configOptions: params.initialized.configOptions ?? params.existingSession?.configOptions ?? [],
    availableCommands: params.availableCommands,
    contextUsage: params.contextUsage ?? params.existingSession?.contextUsage ?? null,
    errorMessage: null,
    blockedReason: null,
    initiative: params.existingSession?.initiative ?? null,
    inboxScope: params.existingSession?.inboxScope ?? null,
    lastAgentMessage: null,
  }
}

/** Persists the records produced by one successful session launch across all daemon-owned kinds. */
export function persistLaunchedSession(
  db: SessionDb,
  params: {
    id: SessionId
    existingSession: SessionDoc | null
    initialTurn: SessionHistoryTurn | null
    existingWorktreeRecord: SessionWorktreeDoc | null
    worktree: PreparedSessionWorktree | null
    sessionRecord: ReturnType<typeof createSessionRecordUpdate>
  },
) {
  if (params.initialTurn) {
    db.sessionTurns.create(toCompletedTurnInput(params.id, params.initialTurn))
  }

  if (params.worktree) {
    const nextWorktree = {
      sessionId: params.id,
      ...params.worktree.state,
    }
    if (params.existingWorktreeRecord) {
      db.worktrees.put(params.existingWorktreeRecord.id, nextWorktree)
    } else {
      db.worktrees.create(nextWorktree)
    }
  }

  if (params.existingSession) {
    const existingDocument = db.sessions.get(params.id) ?? null
    if (!existingDocument) {
      throw new IpcClientError(`Cannot update unknown session: ${params.id}`)
    }

    db.sessions.update(params.id, params.sessionRecord)
    return
  }

  db.sessions.put(params.id, params.sessionRecord)
}
