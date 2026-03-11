import * as acp from "@agentclientprotocol/sdk"
import * as z from "zod"

export interface AgentDistribution {
  type: "binary" | "npx" | "uvx"
  package?: string
  cmd?: string
  args?: string[]
}

interface BaseSessionParams {
  agent: string | AgentDistribution
  cwd: string
  mcpServers: acp.McpServer[]
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
        | { initialPrompt: string | acp.ContentBlock[]; oneShot?: boolean }
      ))

export const SessionServerLog = z.union([
  z.object({
    success: z.literal(true),
    serverAddress: z.string(),
    serverId: z.string(),
    sessionId: z.string(),
  }),
  z.object({
    success: z.literal(false),
    error: z.string(),
  }),
])
export type SessionServerLog = z.infer<typeof SessionServerLog>
