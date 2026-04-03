import * as acp from "@agentclientprotocol/sdk"
import type { DaemonSessionId } from "@goddard-ai/schema/common/params"
import type {
  CreateDaemonSessionRequest,
  CreateDaemonSessionResponse,
  DaemonSession,
  GetDaemonSessionHistoryResponse,
  ListDaemonSessionsRequest,
  ListDaemonSessionsResponse,
  ShutdownDaemonSessionResponse,
} from "@goddard-ai/schema/daemon"

export type {
  CreateDaemonSessionRequest,
  CreateDaemonSessionResponse,
  DaemonSession,
  GetDaemonSessionHistoryResponse,
  ListDaemonSessionsRequest,
  ListDaemonSessionsResponse,
  ShutdownDaemonSessionResponse,
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
