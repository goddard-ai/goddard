import * as acp from "@agentclientprotocol/sdk"
import type { ACPAdapterName } from "@goddard-ai/schema/acp-adapters"
import type {
  CreateDaemonSessionRequest,
  DaemonSession,
  GetDaemonSessionDiagnosticsResponse,
  GetDaemonSessionHistoryResponse,
  ListDaemonSessionsRequest,
  ListDaemonSessionsResponse,
} from "@goddard-ai/schema/daemon"
import { type AgentDistribution } from "@goddard-ai/schema/session-server"
import { type ChildProcessByStdio } from "node:child_process"
import { Readable, Writable } from "node:stream"
/** Describes the concrete child-process invocation for a resolved agent distribution. */
type AgentProcessSpec = {
  cmd: string
  args: string[]
  env?: Record<string, string>
}
/** Exposes the daemon operations for creating, connecting to, and controlling sessions. */
export type SessionManager = {
  createSession: (params: CreateDaemonSessionRequest) => Promise<DaemonSession>
  listSessions: (params: ListDaemonSessionsRequest) => Promise<ListDaemonSessionsResponse>
  connectSession: (id: string) => Promise<DaemonSession>
  getSession: (id: string) => Promise<DaemonSession>
  getHistory: (id: string) => Promise<GetDaemonSessionHistoryResponse>
  getDiagnostics: (id: string) => Promise<GetDaemonSessionDiagnosticsResponse>
  sendMessage: (id: string, message: acp.AnyMessage) => Promise<void>
  promptSession: (id: string, prompt: string | acp.ContentBlock[]) => Promise<acp.PromptResponse>
  shutdownSession: (id: string) => Promise<boolean>
  resolveSessionIdByToken: (token: string) => Promise<string>
  close: () => Promise<void>
}
/** Ensures the daemon's system prompt is prepended to the first user prompt sent to an agent. */
export declare function injectSystemPrompt(
  request: acp.PromptRequest,
  systemPrompt: string,
): acp.PromptRequest
/** Resolves and launches the requested agent distribution for a new daemon session. */
export declare function spawnAgentProcess(
  daemonUrl: string,
  token: string,
  params: {
    agent: ACPAdapterName | AgentDistribution
    cwd: string
    agentBinDir: string
    env?: Record<string, string>
  },
): Promise<ChildProcessByStdio<Writable, Readable, null>>
/** Chooses the concrete command invocation for a resolved agent distribution. */
export declare function resolveAgentProcessSpec(agent: AgentDistribution): Promise<AgentProcessSpec>
/** Creates the daemon-owned session lifecycle boundary over storage and agent processes. */
export declare function createSessionManager(input: {
  daemonUrl: string
  agentBinDir: string
  publish: (id: string, message: acp.AnyMessage) => void
}): SessionManager

//# sourceMappingURL=manager.d.ts.map
