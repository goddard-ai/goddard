import type * as acp from "@agentclientprotocol/sdk"
import type { ACPAdapterName } from "./acp-adapters.js"
import type { SessionStatus } from "./db.js"
import type { AgentDistribution } from "./session-server.js"
import type {
  WorkforceConfig,
  WorkforceProjectionSummary,
  WorkforceRequestIntent,
} from "./workforce.js"

export type DaemonHealth = {
  ok: boolean
}

export type SubmitPrDaemonRequest = {
  cwd: string
  title: string
  body: string
  head?: string
  base?: string
}

export type SubmitPrDaemonResponse = {
  number: number
  url: string
}

export type ReplyPrDaemonRequest = {
  cwd: string
  message: string
  prNumber?: number
}

export type ReplyPrDaemonResponse = {
  success: boolean
}

export type DaemonSessionRuntimeEnv = {
  GODDARD_SESSION_TOKEN: string
}

export type DaemonSessionMetadata = {
  repository?: string
  prNumber?: number
  [key: string]: unknown
}

// Durable connectivity state exposed to app and SDK consumers.
export type DaemonSessionConnection = {
  mode: "live" | "history" | "none"
  reconnectable: boolean
  historyAvailable: boolean
  activeDaemonSession: boolean
}

// Structured diagnostic event emitted by the daemon for session lifecycle debugging.
export type DaemonDiagnosticEvent = {
  type: string
  at: string
  sessionId: string
  detail?: Record<string, unknown>
}

// Lightweight diagnostic summary exposed with every daemon session read.
export type DaemonSessionDiagnostics = {
  eventCount: number
  historyLength: number
  lastEventAt: string | null
}

export type DaemonSessionIdentity = {
  id: string
  acpId: string
}

export type CreateDaemonSessionRequest = {
  agent: ACPAdapterName | AgentDistribution
  cwd: string
  mcpServers: acp.McpServer[]
  systemPrompt: string
  env?: Record<string, string>
  metadata?: DaemonSessionMetadata
  initialPrompt?: string | acp.ContentBlock[]
  oneShot?: boolean
}

export type DaemonSession = DaemonSessionIdentity & {
  status: SessionStatus
  agentName: string
  cwd: string
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

export type CreateDaemonSessionResponse = {
  session: DaemonSession
}

export type DaemonSessionPathParams = {
  id: string
}

export type GetDaemonSessionResponse = {
  session: DaemonSession
}

export type GetDaemonSessionHistoryResponse = DaemonSessionIdentity & {
  connection: DaemonSessionConnection
  history: acp.AnyMessage[]
}

// Full session diagnostic payload returned on demand for debugging and tests.
export type GetDaemonSessionDiagnosticsResponse = DaemonSessionIdentity & {
  connection: DaemonSessionConnection
  events: DaemonDiagnosticEvent[]
}

export type ShutdownDaemonSessionResponse = {
  id: string
  success: boolean
}

// Stable runtime states reported for daemon-managed workforce hosts.
export type DaemonWorkforceRuntimeState = "running"

// Workforce status summary exposed over daemon IPC.
export type DaemonWorkforceStatus = WorkforceProjectionSummary & {
  state: DaemonWorkforceRuntimeState
  rootDir: string
  configPath: string
  ledgerPath: string
}

// One daemon-managed workforce runtime addressed by repository root.
export type DaemonWorkforce = DaemonWorkforceStatus & {
  config: WorkforceConfig
}

// Request payload used to start or reconnect to a daemon-owned workforce.
export type StartDaemonWorkforceRequest = {
  rootDir: string
}

// Request payload used to fetch one daemon-owned workforce by repository root.
export type GetDaemonWorkforceRequest = {
  rootDir: string
}

// Request payload used to stop one daemon-owned workforce by repository root.
export type ShutdownDaemonWorkforceRequest = {
  rootDir: string
}

// Request payload used to enqueue work for one target workforce agent.
export type CreateDaemonWorkforceRequestRequest = {
  rootDir: string
  targetAgentId: string
  input: string
  intent?: WorkforceRequestIntent
  token?: string
}

// Request payload used to add resume context to one workforce request.
export type UpdateDaemonWorkforceRequest = {
  rootDir: string
  requestId: string
  input: string
  token?: string
}

// Request payload used to cancel one workforce request.
export type CancelDaemonWorkforceRequest = {
  rootDir: string
  requestId: string
  reason?: string
  token?: string
}

// Request payload used to clear pending work in one agent scope or the whole runtime.
export type TruncateDaemonWorkforceRequest = {
  rootDir: string
  agentId?: string
  reason?: string
  token?: string
}

// Request payload used by an active workforce agent to finish its current task.
export type RespondDaemonWorkforceRequest = {
  rootDir: string
  requestId: string
  output: string
  token: string
}

// Request payload used by an active workforce agent to suspend its current task.
export type SuspendDaemonWorkforceRequest = {
  rootDir: string
  requestId: string
  reason: string
  token: string
}

// Response payload returned when one workforce runtime is fetched.
export type GetDaemonWorkforceResponse = {
  workforce: DaemonWorkforce
}

// Response payload returned when one workforce runtime is started.
export type StartDaemonWorkforceResponse = {
  workforce: DaemonWorkforce
}

// Response payload returned when all running workforce runtimes are listed.
export type ListDaemonWorkforcesResponse = {
  workforces: DaemonWorkforceStatus[]
}

// Response payload returned after one workforce runtime is stopped.
export type ShutdownDaemonWorkforceResponse = {
  rootDir: string
  success: boolean
}

// Response payload returned after one workforce request mutation.
export type MutateDaemonWorkforceResponse = {
  workforce: DaemonWorkforceStatus
  requestId: string | null
}
