import * as acp from "@agentclientprotocol/sdk"
import { z } from "zod"
import { ACPAdapterName } from "../acp-adapters.ts"
import { DaemonSessionIdParams } from "../common/params.ts"
import { AgentDistribution } from "../session-server/agent-distribution.ts"
import { DaemonSessionMetadata } from "./session-metadata.ts"
import type { SessionStatus } from "../db.ts"

/** Session-start initial prompt values accepted by the daemon session API. */
export const InitialPromptOption = z.union([z.string(), z.array(z.custom<acp.ContentBlock>())])

export type InitialPromptOption = z.infer<typeof InitialPromptOption>

/** Worktree options accepted by the daemon session API. */
export const SessionWorktreeParams = z.object({
  enabled: z.boolean().optional(),
})

export type SessionWorktreeParams = z.infer<typeof SessionWorktreeParams>

/** Request payload used to create one daemon-managed session. */
export const CreateDaemonSessionRequest = z.object({
  agent: z.union([z.string() as z.ZodType<ACPAdapterName>, AgentDistribution]),
  cwd: z.string(),
  worktree: SessionWorktreeParams.optional(),
  mcpServers: z.array(z.custom<acp.McpServer>()),
  systemPrompt: z.string(),
  env: z.record(z.string(), z.string()).optional(),
  repository: z.string().optional(),
  prNumber: z.number().int().optional(),
  metadata: DaemonSessionMetadata.optional(),
  initialPrompt: InitialPromptOption.optional(),
  oneShot: z.boolean().optional(),
})

export type CreateDaemonSessionRequest = z.infer<typeof CreateDaemonSessionRequest>

/** Request payload used to list daemon-managed sessions in stable recency order. */
export const ListDaemonSessionsRequest = z.object({
  limit: z.number().int().positive().optional(),
  cursor: z.string().optional(),
})

export type ListDaemonSessionsRequest = z.infer<typeof ListDaemonSessionsRequest>

/** Path and payload params used to address one daemon-managed session. */
export const DaemonSessionPathParams = DaemonSessionIdParams

export type DaemonSessionPathParams = z.infer<typeof DaemonSessionPathParams>

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
