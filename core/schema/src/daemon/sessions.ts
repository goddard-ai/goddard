import * as acp from "@agentclientprotocol/sdk"
import { z } from "zod"
import { ACPAdapterName } from "../acp-adapters.ts"
import { DaemonSessionIdParams } from "../common/params.ts"
import { AgentDistribution } from "../session-server/agent-distribution.ts"
import { DaemonSessionMetadata } from "./session-metadata.ts"

/** Session-start initial prompt values accepted by the daemon session API. */
export const InitialPromptOption = z.union([z.string(), z.array(z.custom<acp.ContentBlock>())])

export type InitialPromptOption = z.infer<typeof InitialPromptOption>

/** Worktree options accepted by the daemon session API. */
export const SessionWorktreeParams = z.object({
  enabled: z.boolean().optional(),
  existingFolder: z.string().optional(),
})

export type SessionWorktreeParams = z.infer<typeof SessionWorktreeParams>

/** Request payload used to create one daemon-managed session. */
export const CreateDaemonSessionRequest = z.object({
  agent: z.union([z.string() as z.ZodType<ACPAdapterName>, AgentDistribution]),
  cwd: z.string(),
  worktree: SessionWorktreeParams.optional(),
  mcpServers: z.array(z.custom<acp.McpServer>()),
  systemPrompt: z.string(),
  env: z.record(z.string(), z.string()).optional(),
  repository: z.string().optional(),
  prNumber: z.number().int().optional(),
  metadata: DaemonSessionMetadata.optional(),
  initialPrompt: InitialPromptOption.optional(),
  oneShot: z.boolean().optional(),
})

export type CreateDaemonSessionRequest = z.infer<typeof CreateDaemonSessionRequest>

/** Request payload used to list daemon-managed sessions in stable recency order. */
export const ListDaemonSessionsRequest = z.object({
  limit: z.number().int().positive().optional(),
  cursor: z.string().optional(),
})

export type ListDaemonSessionsRequest = z.infer<typeof ListDaemonSessionsRequest>

/** Path and payload params used to address one daemon-managed session. */
export const DaemonSessionPathParams = DaemonSessionIdParams

export type DaemonSessionPathParams = z.infer<typeof DaemonSessionPathParams>
