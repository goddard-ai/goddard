import type * as acp from "@agentclientprotocol/sdk"
import type { ACPAdapterName } from "./acp-adapters.js"
import type { DaemonSessionMetadata } from "./daemon/session-metadata.js"
import type { AgentDistribution } from "./session-server/agent-distribution.js"

export {
  AgentBinaryDistribution,
  AgentBinaryTarget,
  AgentDistribution,
  AgentDistributionEnv,
  AgentInstallationMethods,
  AgentPackageDistribution,
  agentBinaryPlatforms,
} from "./session-server/agent-distribution.js"
export type { AgentBinaryPlatform } from "./session-server/agent-distribution.js"

/** Structured worktree settings accepted when starting one daemon-backed session. */
export interface SessionWorktreeOptions {
  enabled?: boolean
}

/** Shared session creation fields used by both new and reconnect flows. */
interface BaseSessionParams {
  agent: ACPAdapterName | AgentDistribution
  cwd: string
  worktree?: SessionWorktreeOptions
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
  sessionId: string
}

/** Union describing both reconnect and fresh session creation entrypoints. */
export type SessionParams =
  | LoadSessionParams
  | (NewSessionParams &
      (
        | { initialPrompt?: string | acp.ContentBlock[]; oneShot?: undefined }
        | { initialPrompt: string | acp.ContentBlock[]; oneShot: true }
      ))
