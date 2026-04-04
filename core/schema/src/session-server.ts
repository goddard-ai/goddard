import type * as acp from "@agentclientprotocol/sdk"
import type { ACPAdapterName } from "./acp-adapters.ts"
import type { DaemonSessionId } from "./common/params.ts"
import type { DaemonSessionMetadata } from "./daemon/session-metadata.ts"
import type { AgentDistribution } from "./session-server/agent-distribution.ts"

export {
  AgentBinaryDistribution,
  agentBinaryPlatforms,
  AgentBinaryTarget,
  AgentDistribution,
  AgentDistributionEnv,
  AgentInstallationMethods,
  AgentPackageDistribution,
} from "./session-server/agent-distribution.ts"
export type { AgentBinaryPlatform } from "./session-server/agent-distribution.ts"

/** Structured worktree settings accepted when starting one daemon-backed session. */
export interface SessionWorktreeOptions {
  enabled?: boolean
}

/** Structured workforce attachment accepted when starting one daemon-backed session. */
export interface SessionWorkforceOptions {
  rootDir?: string
  agentId?: string
  requestId?: string
  [key: string]: unknown
}

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
