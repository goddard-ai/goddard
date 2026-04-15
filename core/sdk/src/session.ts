/** SDK-owned session helpers and wrapper params for daemon-backed sessions. */
import * as acp from "@agentclientprotocol/sdk"
import type { ACPAdapterName } from "@goddard-ai/schema/acp-adapters"
import type { AgentDistribution } from "@goddard-ai/schema/agent-distribution"
import type { DaemonSessionId } from "@goddard-ai/schema/common/params"
import type {
  CreateSessionRequest,
  CreateSessionResponse,
  DaemonSession,
  SessionComposerSuggestionsRequest,
  SessionComposerSuggestionsResponse,
  DaemonSessionMetadata,
  GetSessionHistoryResponse,
  GetSessionHistoryRequest,
  SessionHistoryMessage,
  SessionHistoryTurn,
  ListSessionsRequest,
  ListSessionsResponse,
  SessionWorkforceParams,
  SessionWorktreeParams,
  ShutdownSessionResponse,
} from "@goddard-ai/schema/daemon"

export type {
  CreateSessionRequest,
  CreateSessionResponse,
  DaemonSession,
  GetSessionHistoryRequest,
  SessionHistoryMessage,
  GetSessionHistoryResponse,
  SessionComposerSuggestionsRequest,
  SessionComposerSuggestionsResponse,
  SessionHistoryTurn,
  ListSessionsRequest,
  ListSessionsResponse,
  ShutdownSessionResponse,
}

export type SessionPromptRequest = {
  id: DaemonSessionId
  acpId: string
  prompt: string | acp.ContentBlock[]
}

export function createSessionPromptMessage(input: SessionPromptRequest) {
  return {
    jsonrpc: "2.0",
    id: crypto.randomUUID(),
    method: acp.AGENT_METHODS.session_prompt,
    params: {
      sessionId: input.acpId,
      prompt:
        typeof input.prompt === "string" ? [{ type: "text", text: input.prompt }] : input.prompt,
    },
  } satisfies acp.AnyMessage
}

/** Structured worktree settings accepted when starting one daemon-backed session. */
export interface SessionWorktreeOptions extends SessionWorktreeParams {}

/** Structured workforce attachment accepted when starting one daemon-backed session. */
export interface SessionWorkforceOptions extends SessionWorkforceParams {}

/** Shared session creation fields used by both new and reconnect flows. */
interface BaseSessionParams {
  agent: ACPAdapterName | AgentDistribution
  cwd: string
  worktree?: SessionWorktreeOptions
  workforce?: SessionWorkforceOptions
  mcpServers: acp.McpServer[]
  systemPrompt?: string
  env?: Record<string, string>
  repository?: string
  prNumber?: number
  metadata?: DaemonSessionMetadata
}

/** Parameters used to create one fresh daemon-backed agent session. */
export interface NewSessionParams extends BaseSessionParams {
  sessionId?: undefined
  initialPrompt?: string | acp.ContentBlock[]
  oneShot?: boolean
}

/** Parameters used to reconnect to one previously created daemon-backed session. */
export interface LoadSessionParams extends BaseSessionParams {
  sessionId: DaemonSessionId
}

/** Union describing both reconnect and fresh session creation entrypoints. */
export type SessionParams =
  | LoadSessionParams
  | (NewSessionParams &
      (
        | { initialPrompt?: string | acp.ContentBlock[]; oneShot?: undefined }
        | { initialPrompt: string | acp.ContentBlock[]; oneShot: true }
      ))
