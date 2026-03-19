import * as acp from "@agentclientprotocol/sdk"
import type { ACPAdapterName } from "./acp-adapters.js"

/** Supported platform keys for ACP binary distributions. */
export const agentBinaryPlatforms = [
  "darwin-aarch64",
  "darwin-x86_64",
  "linux-aarch64",
  "linux-x86_64",
  "windows-aarch64",
  "windows-x86_64",
] as const

/** Supported platform keys for ACP binary distributions. */
export type AgentBinaryPlatform = (typeof agentBinaryPlatforms)[number]

/** Environment variables declared by an ACP distribution target. */
export type AgentDistributionEnv = Record<string, string>

/** Binary execution target metadata for one supported platform. */
export interface AgentBinaryTarget {
  archive: string
  cmd: string
  args?: string[]
  env?: AgentDistributionEnv
}

/** Platform-indexed ACP binary targets. */
export type AgentBinaryDistribution = Partial<Record<AgentBinaryPlatform, AgentBinaryTarget>>

/** Launch metadata for ACP package-based distributions. */
export interface AgentPackageDistribution {
  package: string
  args?: string[]
  env?: AgentDistributionEnv
}

/** Supported ACP distribution methods for one agent entry. */
export interface AgentInstallationMethods {
  binary?: AgentBinaryDistribution
  npx?: AgentPackageDistribution
  uvx?: AgentPackageDistribution
}

/** Structured ACP agent entry accepted by session creation APIs. */
export interface AgentDistribution {
  id: string
  name: string
  version: string
  description: string
  repository?: string
  authors?: string[]
  license?: string
  icon?: string
  distribution: AgentInstallationMethods
}

interface BaseSessionParams {
  agent: ACPAdapterName | AgentDistribution
  cwd: string
  mcpServers: acp.McpServer[]
  systemPrompt?: string
  env?: Record<string, string>
  metadata?: {
    repository?: string
    prNumber?: number
    [key: string]: any
  }
}

export interface NewSessionParams extends BaseSessionParams {
  sessionId?: undefined
  initialPrompt?: string | acp.ContentBlock[]
  oneShot?: boolean
}

export interface LoadSessionParams extends BaseSessionParams {
  sessionId: string
}

export type SessionParams =
  | LoadSessionParams
  | (NewSessionParams &
      (
        | { initialPrompt?: string | acp.ContentBlock[]; oneShot?: undefined }
        | { initialPrompt: string | acp.ContentBlock[]; oneShot: true }
      ))
