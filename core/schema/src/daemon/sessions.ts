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
export const SessionWorktreeParams = z.strictObject({
  enabled: z.boolean().optional(),
})

export type SessionWorktreeParams = z.infer<typeof SessionWorktreeParams>

/** Workforce attachment stored separately from the base daemon session record. */
export const SessionWorkforceParams = z.strictObject({
  rootDir: z.string().optional(),
  agentId: z.string().optional(),
  requestId: z.string().optional(),
})

export type SessionWorkforceParams = z.infer<typeof SessionWorkforceParams>

/** Request payload used to create one daemon-managed session. */
export const CreateSessionRequest = z.strictObject({
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

export type CreateSessionRequest = z.infer<typeof CreateSessionRequest>

/** Request payload used to list daemon-managed sessions in stable recency order. */
export const ListSessionsRequest = z.strictObject({
  limit: z.number().int().positive().optional(),
  cursor: z.string().optional(),
})

export type ListSessionsRequest = z.infer<typeof ListSessionsRequest>

/** Path and payload params used to address one daemon-managed session. */
export const SessionPathParams = DaemonSessionIdParams

export type SessionPathParams = z.infer<typeof SessionPathParams>

/** JSON-RPC request ids surfaced for queued and aborted prompt bookkeeping. */
export const SessionPromptId = z.union([z.string(), z.number()])

export type SessionPromptId = z.infer<typeof SessionPromptId>

/** Request payload used to forward one raw ACP message to a daemon-managed session. */
export const SendSessionMessageRequest = z.strictObject({
  id: DaemonSessionId,
  message: z.unknown(),
})

/** Compile-time shape of one raw ACP message forwarded to a daemon-managed session. */
export interface SendSessionMessageRequest {
  id: DaemonSessionId
  message: acp.AnyMessage
}

/** Request payload used to resolve one daemon session token into its daemon session id. */
export const ResolveSessionTokenRequest = z.strictObject({
  token: z.string(),
})

/** Compile-time shape used to resolve one daemon session token into its daemon session id. */
export type ResolveSessionTokenRequest = z.infer<typeof ResolveSessionTokenRequest>

/** One queued prompt payload surfaced back to clients after daemon-side cancellation. */
export const AbortedSessionPrompt = z.strictObject({
  requestId: SessionPromptId,
  prompt: z.array(z.custom<acp.ContentBlock>()),
})

export type AbortedSessionPrompt = z.infer<typeof AbortedSessionPrompt>

/** Request payload used to cancel the active turn for one daemon-managed session. */
export const CancelSessionRequest = z.strictObject({
  id: DaemonSessionId,
})

export type CancelSessionRequest = z.infer<typeof CancelSessionRequest>

/** Request payload used to cancel the active turn and replace it with one new prompt. */
export const SteerSessionRequest = DaemonSessionIdParams.extend({
  prompt: InitialPromptOption,
})

export type SteerSessionRequest = z.infer<typeof SteerSessionRequest>

/** Stream payload emitted for one daemon-managed ACP session message. */
export const SessionMessageEvent = z.strictObject({
  id: DaemonSessionId,
  message: z.unknown(),
})

/** Compile-time shape of one daemon-managed ACP session message event. */
export interface SessionMessageEvent {
  id: DaemonSessionId
  message: acp.AnyMessage
}

/** Runtime environment variables injected into one daemon-managed session. */
export type SessionRuntimeEnv = {
  GODDARD_SESSION_TOKEN: string
}

/** Durable connectivity state exposed to app and SDK consumers. */
export type SessionConnection = {
  mode: "live" | "history" | "none"
  reconnectable: boolean
  activeDaemonSession: boolean
}

/** Structured diagnostic event emitted by the daemon for session lifecycle debugging. */
export type SessionDiagnosticEvent = DaemonSessionDiagnosticEvent & {
  sessionId: DaemonSessionId
}

/** Stable identity values used to address one daemon-managed session. */
export type SessionIdentity = {
  id: DaemonSessionId
  acpSessionId: string
}

/** Response payload returned after one daemon-managed session is created. */
export type CreateSessionResponse = {
  session: DaemonSession
}

/** Response payload returned after one daemon-managed session page is fetched. */
export type ListSessionsResponse = {
  sessions: DaemonSession[]
  nextCursor: string | null
  hasMore: boolean
}

/** Response payload returned after one daemon-managed session is fetched. */
export type GetSessionResponse = {
  session: DaemonSession
}

/** Response payload returned after one daemon-managed session history fetch. */
export type GetSessionHistoryResponse = SessionIdentity & {
  connection: SessionConnection
  history: acp.AnyMessage[]
}

/** Full session diagnostic payload returned on demand for debugging and tests. */
export type GetSessionDiagnosticsResponse = SessionIdentity & {
  connection: SessionConnection
  events: SessionDiagnosticEvent[]
}

/** Response payload returned after one daemon-managed session worktree fetch. */
export type GetSessionWorktreeResponse = SessionIdentity & {
  worktree: DaemonWorktree | null
}

/** Response payload returned after one daemon-managed session workforce fetch. */
export type GetSessionWorkforceResponse = SessionIdentity & {
  workforce: DaemonWorkforce | null
}

/** Response payload returned after one daemon-managed session shutdown request. */
export type ShutdownSessionResponse = {
  id: DaemonSessionId
  success: boolean
}

/** Response payload returned after one daemon-managed session turn cancellation. */
export type CancelSessionResponse = {
  id: string
  activeTurnCancelled: boolean
  abortedQueue: AbortedSessionPrompt[]
}

/** Response payload returned after one daemon-managed session steer request. */
export type SteerSessionResponse = {
  id: string
  abortedQueue: AbortedSessionPrompt[]
  response: acp.PromptResponse
}
