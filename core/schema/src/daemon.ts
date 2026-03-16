import type * as acp from "@agentclientprotocol/sdk"
import type { ACPAdapterName } from "./acp-adapters.ts"
import type { SessionStatus } from "./db.ts"
import type { AgentDistribution } from "./session-server.ts"

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
  history: acp.AnyMessage[]
}

export type ShutdownDaemonSessionResponse = {
  id: string
  success: boolean
}
