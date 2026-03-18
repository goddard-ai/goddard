import type * as acp from "@agentclientprotocol/sdk"
import type { ACPAdapterName } from "./acp-adapters.js"
import type { SessionStatus } from "./db.js"
import type { AgentDistribution } from "./session-server.js"

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
