import * as acp from "@agentclientprotocol/sdk"
import { z } from "zod"
import { ACPAdapterName } from "../acp-adapters.ts"
import { AgentDistribution } from "../agent-distribution.ts"
import { DaemonSessionId, DaemonSessionIdParams } from "../common/params.ts"
import {
  DaemonSessionMetadata,
  type DaemonSession,
  type DaemonSessionDiagnosticEvent,
  type DaemonWorkforce,
  type DaemonWorktree,
} from "./store.ts"

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

/** JSON-RPC request ids surfaced for queued and aborted prompt bookkeeping. */
export const DaemonSessionPromptId = z.union([z.string(), z.number()])

export type DaemonSessionPromptId = z.infer<typeof DaemonSessionPromptId>

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

/** One queued prompt payload surfaced back to clients after daemon-side cancellation. */
export const AbortedDaemonSessionPrompt = z.object({
  requestId: DaemonSessionPromptId,
  prompt: z.array(z.custom<acp.ContentBlock>()),
})

export type AbortedDaemonSessionPrompt = z.infer<typeof AbortedDaemonSessionPrompt>

/** Request payload used to cancel the active turn for one daemon-managed session. */
export const CancelDaemonSessionRequest = DaemonSessionIdParams

export type CancelDaemonSessionRequest = z.infer<typeof CancelDaemonSessionRequest>

/** Request payload used to cancel the active turn and replace it with one new prompt. */
export const SteerDaemonSessionRequest = DaemonSessionIdParams.extend({
  prompt: InitialPromptOption,
})

export type SteerDaemonSessionRequest = z.infer<typeof SteerDaemonSessionRequest>

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

/** Durable connectivity state exposed to app and SDK consumers. */
export type DaemonSessionConnection = {
  mode: "live" | "history" | "none"
  reconnectable: boolean
  activeDaemonSession: boolean
}

/** Structured diagnostic event emitted by the daemon for session lifecycle debugging. */
export type DaemonDiagnosticEvent = DaemonSessionDiagnosticEvent & {
  sessionId: DaemonSessionId
}

/** Stable identity values used to address one daemon-managed session. */
export type DaemonSessionIdentity = {
  id: DaemonSessionId
  acpSessionId: string
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
  worktree: DaemonWorktree | null
}

/** Response payload returned after one daemon-managed session workforce fetch. */
export type GetDaemonSessionWorkforceResponse = DaemonSessionIdentity & {
  workforce: DaemonWorkforce | null
}

/** Response payload returned after one daemon-managed session shutdown request. */
export type ShutdownDaemonSessionResponse = {
  id: DaemonSessionId
  success: boolean
}

/** Response payload returned after one daemon-managed session turn cancellation. */
export type CancelDaemonSessionResponse = {
  id: string
  activeTurnCancelled: boolean
  abortedQueue: AbortedDaemonSessionPrompt[]
}

/** Response payload returned after one daemon-managed session steer request. */
export type SteerDaemonSessionResponse = {
  id: string
  abortedQueue: AbortedDaemonSessionPrompt[]
  response: acp.PromptResponse
}
