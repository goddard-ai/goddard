import * as acp from "@agentclientprotocol/sdk"
import { z } from "zod"
import { ACPAdapterName } from "../acp-adapters.ts"
import { DaemonSessionId, DaemonSessionIdParams } from "../common/params.ts"
import type { SessionStatus } from "../db.ts"
import { AgentDistribution } from "../session-server/agent-distribution.ts"
import { DaemonSessionMetadata } from "./session-metadata.ts"

/** Session-start initial prompt values accepted by the daemon session API. */
export const InitialPromptOption = z.union([z.string(), z.array(z.custom<acp.ContentBlock>())])

export type InitialPromptOption = z.infer<typeof InitialPromptOption>

/** Worktree options accepted by the daemon session API. */
export const SessionWorktreeParams = z.object({
  enabled: z.boolean().optional(),
})

export type SessionWorktreeParams = z.infer<typeof SessionWorktreeParams>

/** Workforce attachment stored separately from the base daemon session record. */
export const SessionWorkforceParams = z.object({
  rootDir: z.string().optional(),
  agentId: z.string().optional(),
  requestId: z.string().optional(),
})

export type SessionWorkforceParams = z.infer<typeof SessionWorkforceParams>

/** Persisted daemon-managed worktree metadata addressed by one session id. */
export const DaemonSessionWorktree = z.strictObject({
  repoRoot: z.string(),
  requestedCwd: z.string(),
  effectiveCwd: z.string(),
  worktreeDir: z.string(),
  branchName: z.string(),
  poweredBy: z.string(),
})

export type DaemonSessionWorktree = z.infer<typeof DaemonSessionWorktree>

/** Persisted daemon-managed workforce metadata addressed by one session id. */
export const DaemonSessionWorkforce = SessionWorkforceParams

export type DaemonSessionWorkforce = z.infer<typeof DaemonSessionWorkforce>

/** Request payload used to create one daemon-managed session. */
export const CreateDaemonSessionRequest = z.object({
  agent: z.union([z.string() as z.ZodType<ACPAdapterName>, AgentDistribution]),
  cwd: z.string(),
  worktree: SessionWorktreeParams.optional(),
  workforce: SessionWorkforceParams.optional(),
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

/** Request payload used to forward one raw ACP message to a daemon-managed session. */
export const SendDaemonSessionMessageRequest = z.object({
  id: DaemonSessionId,
  message: z.unknown(),
})

/** Compile-time shape of one raw ACP message forwarded to a daemon-managed session. */
export interface SendDaemonSessionMessageRequest {
  id: DaemonSessionId
  message: acp.AnyMessage
}

/** Request payload used to resolve one daemon session token into its daemon session id. */
export const ResolveDaemonSessionTokenRequest = z.object({
  token: z.string(),
})

/** Compile-time shape used to resolve one daemon session token into its daemon session id. */
export type ResolveDaemonSessionTokenRequest = z.infer<typeof ResolveDaemonSessionTokenRequest>

/** Stream payload emitted for one daemon-managed ACP session message. */
export const DaemonSessionMessageEvent = z.object({
  id: DaemonSessionId,
  message: z.unknown(),
})

/** Compile-time shape of one daemon-managed ACP session message event. */
export interface DaemonSessionMessageEvent {
  id: DaemonSessionId
  message: acp.AnyMessage
}

/** Runtime environment variables injected into one daemon-managed session. */
export type DaemonSessionRuntimeEnv = {
  GODDARD_SESSION_TOKEN: string
}

/** Durable PR permission scope stored with one daemon-managed session. */
export type DaemonSessionPermissions = {
  owner: string
  repo: string
  allowedPrNumbers: number[]
}

/** Durable connectivity state exposed to app and SDK consumers. */
export type DaemonSessionConnection = {
  mode: "live" | "history" | "none"
  reconnectable: boolean
  activeDaemonSession: boolean
}

/** Structured diagnostic event emitted by the daemon for session lifecycle debugging. */
export type DaemonDiagnosticEvent = {
  type: string
  at: string
  sessionId: string
  detail?: Record<string, unknown>
}

/** Stable identity values used to address one daemon-managed session. */
export type DaemonSessionIdentity = {
  id: DaemonSessionId
  acpSessionId: string
}

/** Full daemon-managed session record exposed to app and SDK consumers. */
export type DaemonSession = DaemonSessionIdentity & {
  status: SessionStatus
  agentName: string
  cwd: string
  mcpServers: acp.McpServer[]
  connectionMode: "live" | "history" | "none"
  activeDaemonSession: boolean
  token: string | null
  permissions: DaemonSessionPermissions | null
  repository: string | null
  prNumber: number | null
  metadata: DaemonSessionMetadata | null
  createdAt: number
  updatedAt: number
  errorMessage: string | null
  blockedReason: string | null
  initiative: string | null
  lastAgentMessage: string | null
  models: acp.SessionModelState | null
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

/** Response payload returned after one daemon-managed session worktree fetch. */
export type GetDaemonSessionWorktreeResponse = DaemonSessionIdentity & {
  worktree: DaemonSessionWorktree | null
}

/** Response payload returned after one daemon-managed session workforce fetch. */
export type GetDaemonSessionWorkforceResponse = DaemonSessionIdentity & {
  workforce: DaemonSessionWorkforce | null
}

/** Response payload returned after one daemon-managed session shutdown request. */
export type ShutdownDaemonSessionResponse = {
  id: DaemonSessionId
  success: boolean
}
