/** SDK-owned session helpers and wrapper params for daemon-backed sessions. */
import type { AgentDistribution } from "@goddard-ai/schema/agent-distribution"
import type {
  CreateSessionRequest,
  CreateSessionResponse,
  DaemonSession,
  DaemonSessionMetadata,
  GetSessionChangesRequest,
  GetSessionChangesResponse,
  GetSessionHistoryRequest,
  GetSessionHistoryResponse,
  InitialSessionConfigOption,
  ListSessionsRequest,
  ListSessionsResponse,
  ReleaseSessionLaunchLeaseRequest,
  ReleaseSessionLaunchLeaseResponse,
  SessionComposerSuggestionsRequest,
  SessionComposerSuggestionsResponse,
  SessionDraftSuggestionsRequest,
  SessionHistoryMessage,
  SessionHistoryTurn,
  SessionLaunchPreviewRequest,
  SessionLaunchPreviewResponse,
  SessionLocalCheckoutParams,
  SessionSubpackage,
  SessionSubpackagesRequest,
  SessionSubpackagesResponse,
  SessionWorkforceParams,
  SetSessionConfigOptionRequest,
  SetSessionModelRequest,
  ShutdownSessionResponse,
} from "@goddard-ai/schema/daemon"
import type { DaemonSessionId } from "@goddard-ai/schema/id"
import type { SessionWorktreeParams } from "@goddard-ai/session/schema"
import * as acp from "acp-client/protocol"

export type {
  CreateSessionRequest,
  CreateSessionResponse,
  DaemonSession,
  GetSessionChangesRequest,
  GetSessionChangesResponse,
  GetSessionHistoryRequest,
  SessionHistoryMessage,
  GetSessionHistoryResponse,
  InitialSessionConfigOption,
  SessionComposerSuggestionsRequest,
  SessionComposerSuggestionsResponse,
  SessionDraftSuggestionsRequest,
  SessionHistoryTurn,
  SessionLaunchPreviewRequest,
  SessionLaunchPreviewResponse,
  SetSessionConfigOptionRequest,
  SetSessionModelRequest,
  SessionSubpackage,
  SessionSubpackagesRequest,
  SessionSubpackagesResponse,
  SessionWorkforceParams,
  ListSessionsRequest,
  ListSessionsResponse,
  ReleaseSessionLaunchLeaseRequest,
  ReleaseSessionLaunchLeaseResponse,
  SessionLocalCheckoutParams,
  ShutdownSessionResponse,
}
export type { SessionWorktreeParams } from "@goddard-ai/session/schema"

export { deriveSessionLaunchModelConfig } from "./session-launch-model-config.ts"

export type SessionPromptRequest = {
  id: DaemonSessionId
  acpId: string
  prompt: string | acp.ContentBlock[]
}

/** SDK input for answering one ACP permission request through a daemon session. */
export type SessionPermissionResponseRequest = {
  id: DaemonSessionId
  requestId: string | number
  outcome: acp.RequestPermissionOutcome
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

/** Builds the JSON-RPC response frame expected by `session/request_permission`. */
export function createSessionPermissionResponseMessage(input: SessionPermissionResponseRequest) {
  return {
    jsonrpc: "2.0",
    id: input.requestId,
    result: {
      outcome: input.outcome,
    },
  } satisfies acp.AnyMessage
}

/** Shared session creation fields used by both new and reconnect flows. */
interface BaseSessionParams {
  agent?: string | AgentDistribution
  cwd: string
  launchLeaseId?: string
  localCheckout?: SessionLocalCheckoutParams
  worktree?: SessionWorktreeParams
  workforce?: SessionWorkforceParams
  mcpServers: acp.McpServer[]
  systemPrompt?: string
  initialModelId?: string
  initialConfigOptions?: InitialSessionConfigOption[]
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
