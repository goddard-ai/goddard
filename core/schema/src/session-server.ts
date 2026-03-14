import * as acp from "@agentclientprotocol/sdk"
import * as z from "zod"
import type { ACPAdapterName } from "./acp-adapters"

export type {
  /**
   * The namespace for Agent Client Protocol (ACP) types. Re-exported from
   * `@agentclientprotocol/sdk`.
   */
  acp,
}

export interface AgentDistribution {
  type: "binary" | "npx" | "uvx"
  package?: string
  cmd?: string
  args?: string[]
}

type Falsy = false | null | undefined

export type AppendSystemPrompt = string | readonly AppendSystemPrompt[] | Falsy

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
  appendSystemPrompt?: AppendSystemPrompt
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
