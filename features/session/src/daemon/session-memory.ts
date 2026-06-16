import type { DaemonLogger } from "@goddard-ai/daemon-plugin"
import type { AcpClient, AcpSession } from "acp-client"
import type * as acp from "acp-client/protocol"

import type {
  DaemonSession,
  DaemonSessionStatus,
  DaemonSessionTurnDraft,
  SteerSessionResponse,
} from "../schema.ts"
import type { AgentProcessHandle } from "./agent-process.ts"
import type { ActiveTurnBuffer, SessionTurnPromptRequestId } from "./turn-history.ts"

type SessionId = DaemonSession["id"]

/** Represents the most recent permission request awaiting a client decision. */
export type PermissionRequest = {
  id: string
  params: acp.RequestPermissionRequest
  resolve: (response: acp.RequestPermissionResponse) => void
}

/** Queue-backed prompt request owned by the daemon until it is sent or aborted. */
export type QueuedPromptEntry = {
  requestId: string | number
  prompt: acp.ContentBlock[]
  source: "client" | "daemon"
  /** Sequence assigned when the prompt leaves the queue, used for late detached responses. */
  turnSequence?: number
  resolve?: (response: acp.PromptResponse) => void
  reject?: (error: Error) => void
}

/** Deferred steer request waiting for a safe boundary before dispatch. */
export type PendingSteerRequest = {
  requestId: string
  cancelledRequestId: string | number
  prompt: acp.ContentBlock[]
  abortedQueue: SteerSessionResponse["abortedQueue"]
  waitingForBoundary: boolean
  resolve: (response: SteerSessionResponse) => void
  reject: (error: Error) => void
}

/** Holds the live runtime state for a daemon-owned session process. */
export type ActiveSession = {
  id: SessionId
  acpSessionId: string
  logger: DaemonLogger
  token: string
  supportsLoadSession: boolean
  process: AgentProcessHandle
  client: AcpClient
  session: AcpSession
  status: DaemonSessionStatus
  exitCleanup: Promise<void> | null
  nextTurnSequence: number
  activeTurn: ActiveTurnBuffer<DaemonSessionTurnDraft["id"]> | null
  isFirstPrompt: boolean
  systemPrompt: string
  lastPermissionRequest: PermissionRequest | null
  promptQueue: QueuedPromptEntry[]
  blockingPromptRequestId: SessionTurnPromptRequestId | null
  pendingSteer: PendingSteerRequest | null
  idleShutdownTimeoutMs: number
  idleShutdownTimer: ReturnType<typeof setTimeout> | null
}

/** Live daemon memory kept outside durable session records. */
export type SessionMemory = {
  activeSessions: Map<SessionId, ActiveSession>
  activeSessionsByAcpSessionId: Map<string, ActiveSession>
  sessionSubscriberCounts: Map<SessionId, number>
  pendingSessionTitlePreparations: Map<SessionId, Promise<void>>
  pendingSessionTitleGenerations: Map<SessionId, Promise<void>>
}

export function createSessionMemory(): SessionMemory {
  return {
    activeSessions: new Map(),
    activeSessionsByAcpSessionId: new Map(),
    sessionSubscriberCounts: new Map(),
    pendingSessionTitlePreparations: new Map(),
    pendingSessionTitleGenerations: new Map(),
  }
}
