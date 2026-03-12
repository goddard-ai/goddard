import * as acp from "@agentclientprotocol/sdk"
import * as z from "zod"
import type { ACPAdapterName } from "./acp-adapters"

export interface AgentDistribution {
  type: "binary" | "npx" | "uvx"
  package?: string
  cmd?: string
  args?: string[]
}

interface BaseSessionParams {
  agent: ACPAdapterName | AgentDistribution
  cwd: string
  mcpServers: acp.McpServer[]
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
  appendSystemPrompt?: string
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

export const SessionServerLog = z.union([
  z.object({
    success: z.literal(true),
    serverAddress: z.string(),
    serverId: z.string(),
    sessionId: z.string(),
  }),
  z.object({
    success: z.literal(true),
    serverAddress: z.null(),
    serverId: z.null(),
    sessionId: z.string(),
  }),
  z.object({
    success: z.literal(false),
    error: z.string(),
  }),
])
export type SessionServerLog = z.infer<typeof SessionServerLog>
