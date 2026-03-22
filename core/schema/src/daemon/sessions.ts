import * as acp from "@agentclientprotocol/sdk"
import { z } from "zod"
import { ACPAdapterName } from "../acp-adapters.ts"
import { DaemonSessionIdParams } from "../common/params.ts"
import { AgentDistribution } from "../session-server/agent-distribution.ts"
import { DaemonSessionMetadata } from "./session-metadata.ts"

const initialPrompt = z.union([z.string(), z.array(z.custom<acp.ContentBlock>())])
const SessionWorktreeOptions = z.object({
  enabled: z.boolean().optional(),
})

/** Request payload used to create one daemon-managed session. */
export const CreateDaemonSessionRequest = z.object({
  agent: z.union([z.string() as z.ZodType<ACPAdapterName>, AgentDistribution]),
  cwd: z.string(),
  worktree: SessionWorktreeOptions.optional(),
  mcpServers: z.array(z.custom<acp.McpServer>()),
  systemPrompt: z.string(),
  env: z.record(z.string(), z.string()).optional(),
  repository: z.string().optional(),
  prNumber: z.number().int().optional(),
  metadata: DaemonSessionMetadata.optional(),
  initialPrompt: initialPrompt.optional(),
  oneShot: z.boolean().optional(),
})

/** TypeScript shape of the daemon session-creation payload contract. */
export type CreateDaemonSessionRequest = z.infer<typeof CreateDaemonSessionRequest>

/** Request payload used to list daemon-managed sessions in stable recency order. */
export const ListDaemonSessionsRequest = z.object({
  limit: z.number().int().positive().optional(),
  cursor: z.string().optional(),
})

/** TypeScript shape of the daemon session-list payload contract. */
export type ListDaemonSessionsRequest = z.infer<typeof ListDaemonSessionsRequest>

/** Path and payload params used to address one daemon-managed session. */
export const DaemonSessionPathParams = DaemonSessionIdParams

/** TypeScript shape of the path params used to address one daemon-managed session. */
export type DaemonSessionPathParams = z.infer<typeof DaemonSessionPathParams>
