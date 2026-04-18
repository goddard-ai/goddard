import type * as acp from "@agentclientprotocol/sdk"
import { z } from "zod"

import { ACPAdapterName } from "../acp-adapters.ts"
import { AgentDistribution } from "../agent-distribution.ts"
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

export const DaemonSessionTitleState = z.enum([
  "placeholder",
  "fallback",
  "pending",
  "generated",
  "failed",
])

export type DaemonSessionTitleState = z.output<typeof DaemonSessionTitleState>

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

/** One ACP slash command persisted on the daemon session for composer suggestions. */
export const DaemonSessionAvailableCommands = z.custom<acp.AvailableCommand[]>()

export type DaemonSessionAvailableCommands = z.output<typeof DaemonSessionAvailableCommands>

/**
 * Persisted daemon-managed session record stored in kindstore.
 */
export const DaemonSession = z.strictObject({
  acpSessionId: z.string(),
  status: DaemonSessionStatus,
  stopReason: DaemonSessionStopReason.nullable().default(null),
  agent: z
    .union([z.string() as z.ZodType<ACPAdapterName>, AgentDistribution])
    .nullable()
    .default(null),
  agentName: z.string(),
  cwd: z.string(),
  title: z.string().default("New session"),
  titleState: DaemonSessionTitleState.default("placeholder"),
  mcpServers: z.custom<acp.McpServer[]>(),
  connectionMode: DaemonSessionConnectionMode.default("none"),
  supportsLoadSession: z.boolean().default(false),
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
  availableCommands: DaemonSessionAvailableCommands.default([]),
})

export type DaemonSession = z.output<typeof DaemonSession> & {
  id: DaemonSessionId
  createdAt: number
  updatedAt: number
}

/** Stable prompt request id stored on one persisted turn or active-turn draft. */
export const DaemonSessionTurnPromptRequestId = z.union([z.string(), z.number().int()])

export type DaemonSessionTurnPromptRequestId = z.output<typeof DaemonSessionTurnPromptRequestId>

/** Completion category stored for one persisted daemon session turn. */
export const DaemonSessionTurnCompletionKind = z.enum(["result", "error"])

export type DaemonSessionTurnCompletionKind = z.output<typeof DaemonSessionTurnCompletionKind>

/**
 * Persisted completed or interrupted turn stored for one daemon-managed session.
 */
export const DaemonSessionTurn = z.strictObject({
  sessionId: DaemonSessionId,
  turnId: z.string(),
  sequence: z.number().int().nonnegative(),
  promptRequestId: DaemonSessionTurnPromptRequestId,
  startedAt: z.string(),
  completedAt: z.string().nullable().default(null),
  completionKind: DaemonSessionTurnCompletionKind.nullable().default(null),
  stopReason: DaemonSessionStopReason.nullable().default(null),
  messages: z.custom<acp.AnyMessage[]>(),
})

export type DaemonSessionTurn = z.output<typeof DaemonSessionTurn> & {
  id: `trn_${string}`
}

/**
 * Mutable active-turn draft stored while one prompt is still in progress.
 */
export const DaemonSessionTurnDraft = z.strictObject({
  sessionId: DaemonSessionId,
  turnId: z.string(),
  sequence: z.number().int().nonnegative(),
  promptRequestId: DaemonSessionTurnPromptRequestId,
  startedAt: z.string(),
  updatedAt: z.string(),
  messages: z.custom<acp.AnyMessage[]>(),
})

export type DaemonSessionTurnDraft = z.output<typeof DaemonSessionTurnDraft> & {
  id: `drf_${string}`
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
