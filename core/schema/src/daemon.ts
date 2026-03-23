import type * as acp from "@agentclientprotocol/sdk"
import type { DaemonSessionMetadata } from "./daemon/session-metadata.ts"
import type { CreateDaemonSessionRequest } from "./daemon/sessions.ts"
import type { SessionStatus } from "./db.ts"
import type { WorkforceConfig, WorkforceProjectionSummary } from "./workforce.ts"

export type {
  GetDaemonLoopRequest,
  ShutdownDaemonLoopRequest,
  StartDaemonLoopRequest,
} from "./daemon/loops.ts"
export type { ReplyPrDaemonRequest, SubmitPrDaemonRequest } from "./daemon/pull-requests.ts"
export type { DaemonSessionMetadata } from "./daemon/session-metadata.ts"
export type {
  CreateDaemonSessionRequest,
  DaemonSessionPathParams,
  ListDaemonSessionsRequest,
} from "./daemon/sessions.ts"
export type {
  CancelDaemonWorkforceRequest,
  CreateDaemonWorkforceRequestRequest,
  GetDaemonWorkforceRequest,
  RespondDaemonWorkforceRequest,
  ShutdownDaemonWorkforceRequest,
  StartDaemonWorkforceRequest,
  SuspendDaemonWorkforceRequest,
  TruncateDaemonWorkforceRequest,
  UpdateDaemonWorkforceRequest,
} from "./workforce/requests.ts"

/** Minimal daemon liveness payload returned by health probes. */
export type DaemonHealth = {
  ok: boolean
}

/** Response payload returned after one pull request submission. */
export type SubmitPrDaemonResponse = {
  number: number
  url: string
}

/** Response payload returned after one pull request reply. */
export type ReplyPrDaemonResponse = {
  success: boolean
}

/** Runtime environment variables injected into one daemon-managed session. */
export type DaemonSessionRuntimeEnv = {
  GODDARD_SESSION_TOKEN: string
}

/** Durable connectivity state exposed to app and SDK consumers. */
export type DaemonSessionConnection = {
  mode: "live" | "history" | "none"
  reconnectable: boolean
  historyAvailable: boolean
  activeDaemonSession: boolean
}

/** Structured diagnostic event emitted by the daemon for session lifecycle debugging. */
export type DaemonDiagnosticEvent = {
  type: string
  at: string
  sessionId: string
  detail?: Record<string, unknown>
}

/** Lightweight diagnostic summary exposed with every daemon session read. */
export type DaemonSessionDiagnostics = {
  eventCount: number
  historyLength: number
  lastEventAt: string | null
}

/** Stable identity values used to address one daemon-managed session. */
export type DaemonSessionIdentity = {
  id: string
  acpId: string
}

/** Full daemon-managed session record exposed to app and SDK consumers. */
export type DaemonSession = DaemonSessionIdentity & {
  status: SessionStatus
  agentName: string
  cwd: string
  repository: string | null
  prNumber: number | null
  metadata: DaemonSessionMetadata | null
  connection: DaemonSessionConnection
  diagnostics: DaemonSessionDiagnostics
  createdAt: string
  updatedAt: string
  errorMessage: string | null
  blockedReason: string | null
  initiative: string | null
  lastAgentMessage: string | null
}

/** Response payload returned after one daemon-managed session is created. */
export type CreateDaemonSessionResponse = {
  session: DaemonSession
}

/** Response payload returned after one daemon-managed session page is fetched. */
export type ListDaemonSessionsResponse = {
  sessions: DaemonSession[]
  nextCursor: string | null
  hasMore: boolean
}

/** Response payload returned after one daemon-managed session is fetched. */
export type GetDaemonSessionResponse = {
  session: DaemonSession
}

/** Response payload returned after one daemon-managed session history fetch. */
export type GetDaemonSessionHistoryResponse = DaemonSessionIdentity & {
  connection: DaemonSessionConnection
  history: acp.AnyMessage[]
}

/** Full session diagnostic payload returned on demand for debugging and tests. */
export type GetDaemonSessionDiagnosticsResponse = DaemonSessionIdentity & {
  connection: DaemonSessionConnection
  events: DaemonDiagnosticEvent[]
}

/** Response payload returned after one daemon-managed session shutdown request. */
export type ShutdownDaemonSessionResponse = {
  id: string
  success: boolean
}

/** Stable runtime states reported for daemon-managed loop hosts. */
export type DaemonLoopRuntimeState = "running"

/** Resolved session and pacing config owned by one daemon-managed loop runtime. */
export type DaemonLoopConfig = {
  promptModulePath: string
  session: Omit<CreateDaemonSessionRequest, "initialPrompt" | "oneShot">
  rateLimits: {
    cycleDelay: string
    maxOpsPerMinute: number
    maxCyclesBeforePause: number
  }
  retries: {
    maxAttempts: number
    initialDelayMs: number
    maxDelayMs: number
    backoffFactor: number
    jitterRatio: number
  }
}

/** Loop status summary exposed over daemon IPC. */
export type DaemonLoopStatus = {
  state: DaemonLoopRuntimeState
  rootDir: string
  loopName: string
  promptModulePath: string
  startedAt: string
  sessionId: string
  acpId: string
  cycleCount: number
  lastPromptAt: string | null
}

/** One daemon-managed loop runtime addressed by repository root and loop name. */
export type DaemonLoop = DaemonLoopStatus & DaemonLoopConfig

/** Response payload returned when one loop runtime is fetched. */
export type GetDaemonLoopResponse = {
  loop: DaemonLoop
}

/** Response payload returned when one loop runtime is started. */
export type StartDaemonLoopResponse = {
  loop: DaemonLoop
}

/** Response payload returned when all running loop runtimes are listed. */
export type ListDaemonLoopsResponse = {
  loops: DaemonLoopStatus[]
}

/** Response payload returned after one loop runtime is stopped. */
export type ShutdownDaemonLoopResponse = {
  rootDir: string
  loopName: string
  success: boolean
}

/** Stable runtime states reported for daemon-managed workforce hosts. */
export type DaemonWorkforceRuntimeState = "running"

/** Workforce status summary exposed over daemon IPC. */
export type DaemonWorkforceStatus = WorkforceProjectionSummary & {
  state: DaemonWorkforceRuntimeState
  rootDir: string
  configPath: string
  ledgerPath: string
}

/** One daemon-managed workforce runtime addressed by repository root. */
export type DaemonWorkforce = DaemonWorkforceStatus & {
  config: WorkforceConfig
}

/** Response payload returned when one workforce runtime is fetched. */
export type GetDaemonWorkforceResponse = {
  workforce: DaemonWorkforce
}

/** Response payload returned when one workforce runtime is started. */
export type StartDaemonWorkforceResponse = {
  workforce: DaemonWorkforce
}

/** Response payload returned when all running workforce runtimes are listed. */
export type ListDaemonWorkforcesResponse = {
  workforces: DaemonWorkforceStatus[]
}

/** Response payload returned after one workforce runtime is stopped. */
export type ShutdownDaemonWorkforceResponse = {
  rootDir: string
  success: boolean
}

/** Response payload returned after one workforce request mutation. */
export type MutateDaemonWorkforceResponse = {
  workforce: DaemonWorkforceStatus
  requestId: string | null
}
