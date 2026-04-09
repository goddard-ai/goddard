import type * as acp from "@agentclientprotocol/sdk"
import { z } from "zod"
import { DaemonSessionId } from "../common/params.ts"

export const DaemonSessionConnectionMode = z.enum(["live", "history", "none"])

export type DaemonSessionConnectionMode = z.output<typeof DaemonSessionConnectionMode>

export const DaemonSessionStopReason = z.enum([
  "end_turn",
  "max_tokens",
  "max_turn_requests",
  "refusal",
  "cancelled",
])

export type DaemonSessionStopReason = z.output<typeof DaemonSessionStopReason>

export const DaemonSessionStatus = z.enum([
  "idle",
  "active",
  "archived",
  "blocked",
  "done",
  "error",
  "cancelled",
])

export type DaemonSessionStatus = z.output<typeof DaemonSessionStatus>

/**
 * Durable PR permission scope persisted with one daemon-managed session.
 */
export const DaemonSessionPermissions = z.strictObject({
  owner: z.string(),
  repo: z.string(),
  allowedPrNumbers: z.array(z.number().int()),
})

export type DaemonSessionPermissions = z.output<typeof DaemonSessionPermissions>

/** Free-form daemon session metadata shared by session creation contracts. */
export const DaemonSessionMetadata = z
  .object({})
  .catchall(z.unknown())
  .describe("Free-form metadata attached to the daemon session for downstream consumers.")

export type DaemonSessionMetadata = z.infer<typeof DaemonSessionMetadata>

/**
 * Persisted daemon-managed session record stored in kindstore.
 */
export const DaemonSession = z.strictObject({
  acpSessionId: z.string(),
  status: DaemonSessionStatus,
  stopReason: DaemonSessionStopReason.nullable().default(null),
  agentName: z.string(),
  cwd: z.string(),
  mcpServers: z.custom<acp.McpServer[]>(),
  connectionMode: DaemonSessionConnectionMode.default("none"),
  activeDaemonSession: z.boolean().default(false),
  errorMessage: z.string().nullable().default(null),
  blockedReason: z.string().nullable().default(null),
  initiative: z.string().nullable().default(null),
  lastAgentMessage: z.string().nullable().default(null),
  repository: z.string().nullable().default(null),
  prNumber: z.number().int().nullable().default(null),
  token: z.string().nullable().default(null),
  permissions: DaemonSessionPermissions.nullable().default(null),
  metadata: DaemonSessionMetadata.nullable().default(null),
  models: z.custom<acp.SessionModelState>().nullable().default(null),
})

export type DaemonSession = z.output<typeof DaemonSession> & {
  id: DaemonSessionId
  createdAt: number
  updatedAt: number
}

/**
 * Persisted ACP history record stored for one daemon-managed session.
 */
export const DaemonSessionMessages = z.strictObject({
  sessionId: DaemonSessionId,
  messages: z.custom<acp.AnyMessage[]>(),
})

export type DaemonSessionMessages = z.output<typeof DaemonSessionMessages> & {
  id: `msg_${string}`
}

/**
 * Structured diagnostic event persisted for postmortem inspection.
 */
export const DaemonSessionDiagnosticEvent = z.strictObject({
  type: z.string(),
  at: z.string(),
  detail: z.record(z.string(), z.unknown()).optional(),
})

export type DaemonSessionDiagnosticEvent = z.output<typeof DaemonSessionDiagnosticEvent>

/**
 * Persisted diagnostic event record stored for one daemon-managed session.
 */
export const DaemonSessionDiagnostics = z.strictObject({
  sessionId: DaemonSessionId,
  events: z.array(DaemonSessionDiagnosticEvent),
})

export type DaemonSessionDiagnostics = z.output<typeof DaemonSessionDiagnostics> & {
  id: `dgn_${string}`
}

/**
 * Persisted daemon-managed worktree record stored separately from the base session.
 */
export const DaemonWorktree = z.strictObject({
  sessionId: DaemonSessionId,
  repoRoot: z.string(),
  requestedCwd: z.string(),
  effectiveCwd: z.string(),
  worktreeDir: z.string(),
  branchName: z.string(),
  poweredBy: z.string(),
})

export type DaemonWorktree = z.output<typeof DaemonWorktree> & {
  id: `wt_${string}`
}

/**
 * Persisted daemon-managed workforce attachment stored separately from the base session.
 */
export const DaemonWorkforce = z.strictObject({
  sessionId: DaemonSessionId,
  rootDir: z.string().optional(),
  agentId: z.string().optional(),
  requestId: z.string().optional(),
})

export type DaemonWorkforce = z.output<typeof DaemonWorkforce> & {
  id: `wf_${string}`
}

/**
 * Persisted daemon-managed pull request record used for session authorization checks.
 */
export const DaemonPullRequest = z.strictObject({
  host: z.enum(["github"]),
  owner: z.string(),
  repo: z.string(),
  prNumber: z.number().int(),
  cwd: z.string(),
})

export type DaemonPullRequest = z.output<typeof DaemonPullRequest> & {
  id: `pr_${string}`
  updatedAt: number
}
