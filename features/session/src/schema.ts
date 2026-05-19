import type { IpcErrorRegistry, IpcErrorRegistryError } from "@goddard-ai/ipc"
import { AgentDistribution } from "@goddard-ai/schema/agent-distribution"
import { AttentionMetadataInput } from "@goddard-ai/schema/attention"
import { StaticSessionParams as StaticSessionParamsSchema } from "@goddard-ai/schema/config"
import type { AcpAdapterId } from "acp-client"
import * as acp from "acp-client/protocol"
import { textModelConfigSchema, type ModelConfig } from "ai-sdk-json-schema"
import { z } from "zod"

export { StaticSessionParamsSchema as StaticSessionParams }

/** Client-visible daemon session error codes shared across daemon, SDK, and app layers. */
export const SessionErrorCodes = {
  ArchivedNotReconnectable: "session.archived_not_reconnectable",
  CannotCompleteActiveTurn: "session.cannot_complete_active_turn",
  CannotCompleteDirtyWorktree: "session.cannot_complete_dirty_worktree",
  CannotCompleteUnmergedCommits: "session.cannot_complete_unmerged_commits",
  CannotInspectCompletionState: "session.cannot_inspect_completion_state",
  CannotResumeUnsupportedAgent: "session.cannot_resume_unsupported_agent",
  InvalidCursor: "session.invalid_cursor",
  InvalidHistoryCursor: "session.invalid_history_cursor",
  InvalidToken: "session.invalid_token",
  LaunchBareRepository: "session.launch_bare_repository",
  LaunchCheckoutFailed: "session.launch_checkout_failed",
  LaunchDirtyCheckout: "session.launch_dirty_checkout",
  LaunchOutsideRepository: "session.launch_outside_repository",
  MissingJsonRpcId: "session.missing_json_rpc_id",
  NotActive: "session.not_active",
  NotFound: "session.not_found",
  NotReconnectable: "session.not_reconnectable",
  NoWorktree: "session.no_worktree",
  PromptAborted: "session.prompt_aborted",
  UnsupportedMessage: "session.unsupported_message",
} as const

export type SessionErrorCode = (typeof SessionErrorCodes)[keyof typeof SessionErrorCodes]

export const SessionErrorCode = z.enum([
  SessionErrorCodes.ArchivedNotReconnectable,
  SessionErrorCodes.CannotCompleteActiveTurn,
  SessionErrorCodes.CannotCompleteDirtyWorktree,
  SessionErrorCodes.CannotCompleteUnmergedCommits,
  SessionErrorCodes.CannotInspectCompletionState,
  SessionErrorCodes.CannotResumeUnsupportedAgent,
  SessionErrorCodes.InvalidCursor,
  SessionErrorCodes.InvalidHistoryCursor,
  SessionErrorCodes.InvalidToken,
  SessionErrorCodes.LaunchBareRepository,
  SessionErrorCodes.LaunchCheckoutFailed,
  SessionErrorCodes.LaunchDirtyCheckout,
  SessionErrorCodes.LaunchOutsideRepository,
  SessionErrorCodes.MissingJsonRpcId,
  SessionErrorCodes.NotActive,
  SessionErrorCodes.NotFound,
  SessionErrorCodes.NotReconnectable,
  SessionErrorCodes.NoWorktree,
  SessionErrorCodes.PromptAborted,
  SessionErrorCodes.UnsupportedMessage,
])

/** Tagged session id emitted by the session store. */
export const SessionId = z.custom<`ses_${string}`>(
  (value): value is `ses_${string}` => typeof value === "string" && value.startsWith("ses_"),
)

export type SessionId = z.infer<typeof SessionId>

/** Stable path and payload params used to address one session by id. */
export const SessionIdParams = z.strictObject({
  id: SessionId,
})

export type SessionIdParams = z.infer<typeof SessionIdParams>

export const DaemonSessionConnectionMode = z.enum(["live", "history", "none"])

export type DaemonSessionConnectionMode = z.output<typeof DaemonSessionConnectionMode>

/** Structured client-visible daemon session errors keyed by exported error identifiers. */
export const SessionIpcErrors = {
  ArchivedNotReconnectable: {
    code: SessionErrorCodes.ArchivedNotReconnectable,
    details: z.strictObject({
      connectionMode: DaemonSessionConnectionMode,
      sessionId: SessionId,
    }),
  },
  CannotCompleteActiveTurn: {
    code: SessionErrorCodes.CannotCompleteActiveTurn,
    details: z.strictObject({
      sessionId: SessionId,
    }),
  },
  CannotCompleteDirtyWorktree: {
    code: SessionErrorCodes.CannotCompleteDirtyWorktree,
    details: z.strictObject({
      sessionId: SessionId,
      worktreeDir: z.string(),
    }),
  },
  CannotCompleteUnmergedCommits: {
    code: SessionErrorCodes.CannotCompleteUnmergedCommits,
    details: z.strictObject({
      sessionId: SessionId,
      worktreeDir: z.string(),
    }),
  },
  CannotInspectCompletionState: {
    code: SessionErrorCodes.CannotInspectCompletionState,
    details: z.strictObject({
      sessionId: SessionId,
    }),
  },
  CannotResumeUnsupportedAgent: {
    code: SessionErrorCodes.CannotResumeUnsupportedAgent,
    details: z.strictObject({
      acpSessionId: z.string(),
    }),
  },
  InvalidCursor: {
    code: SessionErrorCodes.InvalidCursor,
    details: z.strictObject({
      cursor: z.string().nullable(),
    }),
  },
  InvalidHistoryCursor: {
    code: SessionErrorCodes.InvalidHistoryCursor,
    details: z.strictObject({
      cursor: z.string().nullable(),
      sessionId: SessionId,
    }),
  },
  InvalidToken: {
    code: SessionErrorCodes.InvalidToken,
    details: z.undefined(),
  },
  LaunchBareRepository: {
    code: SessionErrorCodes.LaunchBareRepository,
    details: z.strictObject({
      cwd: z.string(),
    }),
  },
  LaunchCheckoutFailed: {
    code: SessionErrorCodes.LaunchCheckoutFailed,
    details: z.strictObject({
      branchName: z.string(),
      cwd: z.string(),
      repoRoot: z.string(),
    }),
  },
  LaunchDirtyCheckout: {
    code: SessionErrorCodes.LaunchDirtyCheckout,
    details: z.strictObject({
      cwd: z.string(),
      repoRoot: z.string(),
    }),
  },
  LaunchOutsideRepository: {
    code: SessionErrorCodes.LaunchOutsideRepository,
    details: z.strictObject({
      cwd: z.string(),
    }),
  },
  MissingJsonRpcId: {
    code: SessionErrorCodes.MissingJsonRpcId,
    details: z.strictObject({
      sessionId: SessionId,
    }),
  },
  NotActive: {
    code: SessionErrorCodes.NotActive,
    details: z.strictObject({
      sessionId: SessionId,
    }),
  },
  NotFound: {
    code: SessionErrorCodes.NotFound,
    details: z.strictObject({
      sessionId: SessionId,
    }),
  },
  NotReconnectable: {
    code: SessionErrorCodes.NotReconnectable,
    details: z.strictObject({
      connectionMode: DaemonSessionConnectionMode,
      sessionId: SessionId,
    }),
  },
  NoWorktree: {
    code: SessionErrorCodes.NoWorktree,
    details: z.strictObject({
      sessionId: SessionId,
    }),
  },
  PromptAborted: {
    code: SessionErrorCodes.PromptAborted,
    details: z.strictObject({
      sessionId: SessionId,
    }),
  },
  UnsupportedMessage: {
    code: SessionErrorCodes.UnsupportedMessage,
    details: z.strictObject({
      sessionId: SessionId,
    }),
  },
} as const satisfies IpcErrorRegistry

export type SessionIpcError = IpcErrorRegistryError<typeof SessionIpcErrors>

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

/** Latest ACP config options reported by the agent for one daemon session. */
export const DaemonSessionConfigOptions = z.custom<acp.SessionConfigOption[]>().default([])

export type DaemonSessionConfigOptions = z.output<typeof DaemonSessionConfigOptions>

/** Latest context window usage reported by the agent for one daemon session. */
export const DaemonSessionContextUsage = z
  .strictObject({
    size: z.number().finite().positive(),
    used: z.number().finite().nonnegative(),
  })
  .nullable()
  .default(null)

export type DaemonSessionContextUsage = z.output<typeof DaemonSessionContextUsage>

/**
 * Persisted daemon-managed session record stored in kindstore.
 */
export const DaemonSession = z.strictObject({
  acpSessionId: z.string(),
  status: DaemonSessionStatus,
  stopReason: DaemonSessionStopReason.nullable().default(null),
  agent: z
    .union([z.string() as z.ZodType<AcpAdapterId>, AgentDistribution])
    .nullable()
    .default(null),
  agentName: z.string(),
  cwd: z.string(),
  title: z.string().default("New session"),
  titleState: DaemonSessionTitleState.default("placeholder"),
  lastSessionActivityAt: z.number().int(),
  mcpServers: z.custom<acp.McpServer[]>(),
  connectionMode: DaemonSessionConnectionMode.default("none"),
  supportsLoadSession: z.boolean().default(false),
  activeDaemonSession: z.boolean().default(false),
  completedHidden: z.boolean().default(false),
  errorMessage: z.string().nullable().default(null),
  blockedReason: z.string().nullable().default(null),
  initiative: z.string().nullable().default(null),
  inboxScope: z.string().nullable().optional().default(null),
  lastAgentMessage: z.string().nullable().default(null),
  repository: z.string().nullable().default(null),
  prNumber: z.number().int().nullable().default(null),
  token: z.string().nullable().default(null),
  permissions: DaemonSessionPermissions.nullable().default(null),
  metadata: DaemonSessionMetadata.nullable().default(null),
  configOptions: DaemonSessionConfigOptions,
  availableCommands: DaemonSessionAvailableCommands.default([]),
  contextUsage: DaemonSessionContextUsage,
})

export type DaemonSession = z.output<typeof DaemonSession> & {
  id: SessionId
  createdAt: number
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
  sessionId: SessionId,
  turnId: z.string(),
  sequence: z.number().int().nonnegative(),
  promptRequestId: DaemonSessionTurnPromptRequestId,
  startedAt: z.string(),
  completedAt: z.string().nullable().default(null),
  completionKind: DaemonSessionTurnCompletionKind.nullable().default(null),
  stopReason: DaemonSessionStopReason.nullable().default(null),
  inboxScope: z.string().nullable().optional().default(null),
  inboxHeadline: z.string().nullable().optional().default(null),
  messages: z.custom<SessionTurnMessage[]>(),
})

export type DaemonSessionTurn = z.output<typeof DaemonSessionTurn> & {
  id: `trn_${string}`
}

/**
 * Mutable active-turn draft stored while one prompt is still in progress.
 */
export const DaemonSessionTurnDraft = z.strictObject({
  sessionId: SessionId,
  turnId: z.string(),
  sequence: z.number().int().nonnegative(),
  promptRequestId: DaemonSessionTurnPromptRequestId,
  startedAt: z.string(),
  updatedAt: z.string(),
  messages: z.custom<SessionTurnMessage[]>(),
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
  sessionId: SessionId,
  events: z.array(DaemonSessionDiagnosticEvent),
})

export type DaemonSessionDiagnostics = z.output<typeof DaemonSessionDiagnostics> & {
  id: `dgn_${string}`
}

/**
 * Persisted daemon-managed worktree record stored separately from the base session.
 */
export const DaemonWorktree = z.strictObject({
  sessionId: SessionId,
  repoRoot: z.string(),
  requestedCwd: z.string(),
  effectiveCwd: z.string(),
  worktreeDir: z.string(),
  branchName: z.string(),
  mergeTargetBranch: z.string().nullable().default(null),
  poweredBy: z.string(),
})

export type DaemonWorktree = z.output<typeof DaemonWorktree> & {
  id: `wt_${string}`
}

/**
 * Persisted launch-dialog worktree prepared before durable session creation.
 */
export const DaemonLaunchWorktree = z.strictObject({
  key: z.string(),
  repoRoot: z.string(),
  requestedCwd: z.string(),
  effectiveCwd: z.string(),
  worktreeDir: z.string(),
  branchName: z.string(),
  mergeTargetBranch: z.string().nullable().default(null),
  poweredBy: z.string(),
  releaseAfter: z.string().nullable(),
})

export type DaemonLaunchWorktree = z.output<typeof DaemonLaunchWorktree> & {
  id: `lwt_${string}`
}

/** Session-start initial prompt values accepted by the daemon session API. */
export const InitialPromptOption = z.union([z.string(), z.array(z.custom<acp.ContentBlock>())])

export type InitialPromptOption = z.infer<typeof InitialPromptOption>

/** Local-checkout branch switching options accepted by the daemon session API. */
export const SessionLocalCheckoutParams = z.strictObject({
  branchName: z.string(),
})

export type SessionLocalCheckoutParams = z.infer<typeof SessionLocalCheckoutParams>

/** Worktree options accepted by the daemon session API. */
export const CreateSessionWorktreeParams = z.strictObject({
  enabled: z.boolean().optional(),
  baseBranchName: z.string().optional(),
})

export type CreateSessionWorktreeParams = z.infer<typeof CreateSessionWorktreeParams>

/** Request payload used to create one daemon-managed session. */
export const InitialSessionConfigOption = z.union([
  z.strictObject({
    configId: z.string(),
    type: z.literal("boolean"),
    value: z.boolean(),
  }),
  z.strictObject({
    configId: z.string(),
    value: z.string(),
  }),
])

export type InitialSessionConfigOption = z.infer<typeof InitialSessionConfigOption>

/** Request payload used to create one daemon-managed session. */
export const CreateSessionRequest = z.strictObject({
  agent: z.union([z.string() as z.ZodType<AcpAdapterId>, AgentDistribution]).optional(),
  cwd: z.string(),
  launchLeaseId: z.string().optional(),
  launchWorktreeId: z.string().optional(),
  localCheckout: SessionLocalCheckoutParams.optional(),
  worktree: CreateSessionWorktreeParams.optional(),
  mcpServers: z.array(z.custom<acp.McpServer>()),
  systemPrompt: z.string().optional(),
  initialModelId: z.string().optional(),
  initialConfigOptions: z.array(InitialSessionConfigOption).optional(),
  env: z.record(z.string(), z.string()).optional(),
  repository: z.string().optional(),
  prNumber: z.number().int().optional(),
  metadata: DaemonSessionMetadata.optional(),
  initialPrompt: InitialPromptOption.optional(),
  oneShot: z.boolean().optional(),
})

export type CreateSessionRequest = z.infer<typeof CreateSessionRequest>

/** Request payload used to list daemon-managed sessions in stable recency order. */
export const ListSessionsRequest = z.strictObject({
  limit: z.number().int().positive().optional(),
  cursor: z.string().optional(),
})

export type ListSessionsRequest = z.infer<typeof ListSessionsRequest>

/** Path and payload params used to address one daemon-managed session. */
export const SessionPathParams = SessionIdParams

export type SessionPathParams = z.infer<typeof SessionPathParams>

/** Request payload used to read one page of daemon-managed session turn history. */
export const GetSessionHistoryRequest = SessionIdParams.extend({
  cursor: z.string().optional(),
  limit: z.number().int().positive().optional(),
})

export type GetSessionHistoryRequest = z.infer<typeof GetSessionHistoryRequest>

/** Request payload used to read the current git diff for one daemon-managed session workspace. */
export const GetSessionChangesRequest = SessionIdParams

export type GetSessionChangesRequest = z.infer<typeof GetSessionChangesRequest>

/** Trigger categories supported by the session chat composer suggestion API. */
export const SessionComposerSuggestionTrigger = z.enum(["dollar", "slash"])

export type SessionComposerSuggestionTrigger = z.infer<typeof SessionComposerSuggestionTrigger>

/** Trigger categories supported by the launch dialog composer suggestion API. */
export const SessionDraftSuggestionTrigger = z.enum(["dollar"])

export type SessionDraftSuggestionTrigger = z.infer<typeof SessionDraftSuggestionTrigger>

/** Request payload used to read session-scoped composer suggestions for one trigger. */
export const SessionComposerSuggestionsRequest = SessionIdParams.extend({
  trigger: SessionComposerSuggestionTrigger,
  query: z.string(),
  limit: z.number().int().positive().optional(),
})

export type SessionComposerSuggestionsRequest = z.infer<typeof SessionComposerSuggestionsRequest>

/** Request payload used to read draft composer suggestions before a session exists. */
export const SessionDraftSuggestionsRequest = z.strictObject({
  cwd: z.string(),
  trigger: SessionDraftSuggestionTrigger,
  query: z.string(),
  limit: z.number().int().positive().optional(),
})

export type SessionDraftSuggestionsRequest = z.infer<typeof SessionDraftSuggestionsRequest>

/** Skill-backed suggestion item returned for one `$` trigger lookup. */
export const SessionComposerSkillSuggestionSource = z.enum(["local", "global"])

export type SessionComposerSkillSuggestionSource = z.infer<
  typeof SessionComposerSkillSuggestionSource
>

/** Skill-backed suggestion item returned for one `$` trigger lookup. */
export const SessionComposerSkillSuggestion = z.strictObject({
  type: z.literal("skill"),
  path: z.string(),
  uri: z.string(),
  label: z.string(),
  detail: z.string(),
  source: SessionComposerSkillSuggestionSource,
})

export type SessionComposerSkillSuggestion = z.infer<typeof SessionComposerSkillSuggestion>

/** Slash-command suggestion item returned for one `/` trigger lookup. */
export const SessionComposerSlashCommandSuggestion = z.strictObject({
  type: z.literal("slash_command"),
  name: z.string(),
  description: z.string(),
  inputHint: z.string().nullable().optional(),
})

export type SessionComposerSlashCommandSuggestion = z.infer<
  typeof SessionComposerSlashCommandSuggestion
>

/** One suggestion item returned for the session chat composer. */
export const SessionComposerSuggestion = z.union([
  SessionComposerSkillSuggestion,
  SessionComposerSlashCommandSuggestion,
])

export type SessionComposerSuggestion = z.infer<typeof SessionComposerSuggestion>

/** Response payload returned after reading session-scoped composer suggestions. */
export const SessionComposerSuggestionsResponse = z.strictObject({
  suggestions: z.array(SessionComposerSuggestion),
})

export type SessionComposerSuggestionsResponse = z.infer<typeof SessionComposerSuggestionsResponse>

/** Request payload used to inspect launch-time adapter and repository capabilities. */
export const SessionLaunchPreviewRequest = z.strictObject({
  agent: z.union([z.string() as z.ZodType<AcpAdapterId>, AgentDistribution]),
  cwd: z.string(),
})

export type SessionLaunchPreviewRequest = z.infer<typeof SessionLaunchPreviewRequest>

/** Request payload used to release a prepared launch lease after launch-dialog abandonment. */
export const ReleaseSessionLaunchLeaseRequest = z.strictObject({
  launchLeaseId: z.string(),
})

export type ReleaseSessionLaunchLeaseRequest = z.infer<typeof ReleaseSessionLaunchLeaseRequest>

/** Response payload returned after a launch lease release has been scheduled. */
export type ReleaseSessionLaunchLeaseResponse = {
  launchLeaseId: string
  released: boolean
}

/** Request payload used to prepare a launch-dialog worktree before session creation. */
export const PrepareSessionLaunchWorktreeRequest = z.strictObject({
  cwd: z.string(),
  baseBranchName: z.string().optional(),
})

export type PrepareSessionLaunchWorktreeRequest = z.infer<
  typeof PrepareSessionLaunchWorktreeRequest
>

/** Response payload returned after launch-dialog worktree preparation. */
export type PrepareSessionLaunchWorktreeResponse = {
  launchWorktreeId: string | null
  worktree: SessionWorktree | null
}

/** Request payload used to release a launch-dialog worktree after abandonment. */
export const ReleaseSessionLaunchWorktreeRequest = z.strictObject({
  launchWorktreeId: z.string(),
})

export type ReleaseSessionLaunchWorktreeRequest = z.infer<
  typeof ReleaseSessionLaunchWorktreeRequest
>

/** Response payload returned after launch-dialog worktree release is scheduled. */
export type ReleaseSessionLaunchWorktreeResponse = {
  launchWorktreeId: string
  released: boolean
}

/** Request payload used to discover launchable subpackage directories under one project cwd. */
export const SessionSubpackagesRequest = z.strictObject({
  cwd: z.string(),
})

export type SessionSubpackagesRequest = z.infer<typeof SessionSubpackagesRequest>

/** One package-boundary directory selectable as a session working directory. */
export type SessionSubpackage = {
  path: string
  relativePath: string
  name: string
  manifestPath: string
}

/** Response payload returned after discovering launchable subpackage directories. */
export type SessionSubpackagesResponse = {
  subpackages: SessionSubpackage[]
}

/** Response payload returned after loading launch-time adapter and repository capabilities. */
export type SessionLaunchPreviewResponse = {
  launchLeaseId: string
  repoRoot: string | null
  bare: boolean
  branches: string[]
  currentBranch: string | null
  dirty: boolean
  configOptions: acp.SessionConfigOption[]
  slashCommands: SessionComposerSlashCommandSuggestion[]
}

/** JSON-RPC request ids surfaced for queued and aborted prompt bookkeeping. */
export const SessionPromptId = z.union([z.string(), z.number()])

export type SessionPromptId = z.infer<typeof SessionPromptId>

/** Request payload used to forward one raw ACP message to a daemon-managed session. */
export const SendSessionMessageRequest = z.strictObject({
  id: SessionId,
  message: z.unknown(),
})

/** Compile-time shape of one raw ACP message forwarded to a daemon-managed session. */
export interface SendSessionMessageRequest {
  id: SessionId
  message: acp.AnyMessage
}

/** Request payload used to resolve one daemon session token into its daemon session id. */
export const ResolveSessionTokenRequest = z.strictObject({
  token: z.string(),
})

/** Compile-time shape used to resolve one daemon session token into its daemon session id. */
export type ResolveSessionTokenRequest = z.infer<typeof ResolveSessionTokenRequest>

/** One queued prompt payload surfaced back to clients after daemon-side cancellation. */
export const AbortedSessionPrompt = z.strictObject({
  requestId: SessionPromptId,
  prompt: z.array(z.custom<acp.ContentBlock>()),
})

export type AbortedSessionPrompt = z.infer<typeof AbortedSessionPrompt>

/** One queued prompt payload removed before daemon dispatch. */
export type QueuedSessionPrompt = AbortedSessionPrompt

/** Response payload returned after removing one queued client prompt before dispatch. */
export type PopQueuedSessionPromptResponse = {
  id: string
  prompt: QueuedSessionPrompt | null
}

/** Request payload used to cancel the active turn for one daemon-managed session. */
export const CancelSessionRequest = z.strictObject({
  id: SessionId,
})

export type CancelSessionRequest = z.infer<typeof CancelSessionRequest>

/** Request payload used to cancel the active turn and replace it with one new prompt. */
export const SteerSessionRequest = SessionIdParams.extend({
  prompt: InitialPromptOption,
})

export type SteerSessionRequest = z.infer<typeof SteerSessionRequest>

/** Request payload used to update one ACP session config option. */
export const SetSessionConfigOptionRequest = SessionIdParams.extend({
  configId: z.string(),
  value: z.string(),
})

export type SetSessionConfigOptionRequest = z.infer<typeof SetSessionConfigOptionRequest>

/** Request payload used to update the active ACP session model. */
export const SetSessionModelRequest = SessionIdParams.extend({
  modelId: z.string(),
})

export type SetSessionModelRequest = z.infer<typeof SetSessionModelRequest>

/** Request payload used to declare the current initiative for one daemon session. */
export const DeclareSessionInitiativeRequest = SessionIdParams.extend({
  title: z.string().trim().min(1),
})

export type DeclareSessionInitiativeRequest = z.infer<typeof DeclareSessionInitiativeRequest>

/** Request payload used to report that one daemon session is blocked. */
export const ReportSessionBlockerRequest = SessionIdParams.extend({
  reason: z.string().trim().min(1),
  scope: AttentionMetadataInput.shape.scope,
  headline: AttentionMetadataInput.shape.headline,
})

export type ReportSessionBlockerRequest = z.infer<typeof ReportSessionBlockerRequest>

/** Request payload used to report the end of one daemon session turn. */
export const ReportSessionTurnEndedRequest = SessionIdParams.extend({
  scope: AttentionMetadataInput.shape.scope,
  headline: AttentionMetadataInput.shape.headline,
})

export type ReportSessionTurnEndedRequest = z.infer<typeof ReportSessionTurnEndedRequest>

/** Request payload used to mark one session's inbox row as completed. */
export const CompleteSessionRequest = SessionIdParams

export type CompleteSessionRequest = z.infer<typeof CompleteSessionRequest>

/** Response payload returned after one session reporting mutation. */
export type ReportSessionResponse = {
  session: DaemonSession
}

/** Response payload returned after updating one ACP session config option. */
export type SetSessionConfigOptionResponse = {
  session: DaemonSession
}

/** Response payload returned after updating the active ACP session model. */
export type SetSessionModelResponse = {
  session: DaemonSession
}

/** Response payload returned after marking one session completed. */
export type CompleteSessionResponse = {
  session: DaemonSession
}

/** One raw ACP message annotated with its stable sequence position inside a turn. */
export const SessionTurnMessage = z.strictObject({
  sequence: z.number().int().nonnegative(),
  sequenceStart: z.number().int().nonnegative(),
  message: z.custom<acp.AnyMessage>(),
})

/** One raw ACP message annotated with its stable sequence position inside a turn. */
export interface SessionTurnMessage {
  /** Highest raw message sequence covered by this payload. */
  sequence: number
  /** Lowest raw message sequence covered by this payload. */
  sequenceStart: number
  /** Original ACP message payload carried at this turn position. */
  message: acp.AnyMessage
}

/** Returns true when a sequenced turn message covers one raw turn-message sequence. */
export function sessionTurnMessageCoversSequence(
  message: Pick<SessionTurnMessage, "sequence" | "sequenceStart">,
  sequence: number,
) {
  return sequence >= message.sequenceStart && sequence <= message.sequence
}

/** Non-history ACP usage update emitted on the live message stream. */
export type SessionUsageUpdateMessage = {
  jsonrpc: "2.0"
  method: typeof acp.CLIENT_METHODS.session_update
  params: acp.SessionNotification & {
    update: acp.SessionUpdate & {
      sessionUpdate: "usage_update"
      size: number
      used: number
    }
  }
}

/** Stream payload emitted for one daemon-managed ACP session message. */
export const SessionMessageEvent = z.custom<SessionUsageUpdateMessage | SessionTurnMessage>()

export type SessionMessageEvent = z.infer<typeof SessionMessageEvent>

/** Session record or runtime area affected by one app-wide lifecycle event. */
export const SessionLifecycleField = z.enum([
  "status",
  "connection",
  "activeTurn",
  "queue",
  "permission",
  "title",
  "contextUsage",
  "lastAgentMessage",
  "lastSessionActivity",
  "completedHidden",
])

export type SessionLifecycleField = z.infer<typeof SessionLifecycleField>

/** Stream payload emitted for app-wide daemon session lifecycle updates. */
export const SessionLifecycleEvent = z.union([
  z.strictObject({
    kind: z.literal("sessionUpdated"),
    session: DaemonSession.extend({
      id: SessionId,
      createdAt: z.number(),
    }),
    changed: z.array(SessionLifecycleField),
  }),
  z.strictObject({
    kind: z.literal("sessionDeleted"),
    id: SessionId,
  }),
])

export type SessionLifecycleEvent =
  | {
      kind: "sessionUpdated"
      session: DaemonSession
      changed: SessionLifecycleField[]
    }
  | {
      kind: "sessionDeleted"
      id: SessionId
    }

/** Runtime environment variables injected into one daemon-managed session. */
export type SessionRuntimeEnv = {
  GODDARD_SESSION_TOKEN: string
}

/** Durable connectivity state exposed to app and SDK consumers. */
export type SessionConnection = {
  mode: "live" | "history" | "none"
  reconnectable: boolean
  activeDaemonSession: boolean
}

/** Structured diagnostic event emitted by the daemon for session lifecycle debugging. */
export type SessionDiagnosticEvent = DaemonSessionDiagnosticEvent & {
  sessionId: SessionId
}

/** Stable identity values used to address one daemon-managed session. */
export type SessionIdentity = {
  id: SessionId
  acpSessionId: string
}

/** Response payload returned after one daemon-managed session is created. */
export type CreateSessionResponse = {
  session: DaemonSession
}

/** Response payload returned after one daemon-managed session page is fetched. */
export type ListSessionsResponse = {
  sessions: DaemonSession[]
  nextCursor: string | null
  hasMore: boolean
}

/** Response payload returned after one daemon-managed session is fetched. */
export type GetSessionResponse = {
  session: DaemonSession
}

/** One persisted or in-progress prompt turn returned by the session history API. */
export const SessionHistoryTurn = z.strictObject({
  turnId: z.string(),
  sequence: z.number().int().nonnegative(),
  promptRequestId: DaemonSessionTurnPromptRequestId,
  startedAt: z.string(),
  completedAt: z.string().nullable(),
  completionKind: DaemonSessionTurnCompletionKind.nullable(),
  stopReason: DaemonSessionStopReason.nullable(),
  inboxScope: z.string().nullable().optional().default(null),
  inboxHeadline: z.string().nullable().optional().default(null),
  messages: z.custom<SessionTurnMessage[]>(),
})

/** One persisted or in-progress prompt turn returned by the session history API. */
export interface SessionHistoryTurn {
  turnId: string
  sequence: number
  promptRequestId: string | number
  startedAt: string
  completedAt: string | null
  completionKind: "result" | "error" | null
  stopReason: z.output<typeof DaemonSessionStopReason> | null
  inboxScope: string | null
  inboxHeadline: string | null
  messages: SessionTurnMessage[]
}

/** One ACP message carried inside a session history turn. */
export type SessionHistoryMessage = SessionHistoryTurn["messages"][number]

/** Response payload returned after one daemon-managed session history fetch. */
export type GetSessionHistoryResponse = SessionIdentity & {
  connection: SessionConnection
  turns: SessionHistoryTurn[]
  nextCursor: string | null
  hasMore: boolean
}

/** Response payload returned after one daemon-managed session git-diff fetch. */
export type GetSessionChangesResponse = SessionIdentity & {
  workspaceRoot: string | null
  diff: string
  hasChanges: boolean
}

/** Full session diagnostic payload returned on demand for debugging and tests. */
export type GetSessionDiagnosticsResponse = SessionIdentity & {
  connection: SessionConnection
  events: SessionDiagnosticEvent[]
}

/** Response payload returned after one daemon-managed session shutdown request. */
export type ShutdownSessionResponse = {
  id: SessionId
  success: boolean
}

/** Response payload returned after one daemon-managed session turn cancellation. */
export type CancelSessionResponse = {
  id: string
  activeTurnCancelled: boolean
  abortedQueue: AbortedSessionPrompt[]
}

/** Response payload returned after one daemon-managed session steer request. */
export type SteerSessionResponse = {
  id: string
  abortedQueue: AbortedSessionPrompt[]
  response: acp.PromptResponse
}

/** Schema for one custom worktree plugin loaded from a filesystem path. */
export const WorktreePluginPathReference = z
  .strictObject({
    type: z.literal("path"),
    path: z
      .string()
      .min(1)
      .describe(
        "Absolute plugin module path or a path resolved relative to the Goddard global directory.",
      ),
    export: z
      .string()
      .min(1)
      .optional()
      .describe("Optional module export name to load. Defaults to `default`."),
  })
  .describe("Reference to a custom worktree plugin loaded from a module path.")

export type WorktreePluginPathReference = z.infer<typeof WorktreePluginPathReference>

/** Schema for one custom worktree plugin loaded from a globally installed package. */
export const WorktreePluginPackageReference = z
  .strictObject({
    type: z.literal("package"),
    package: z
      .string()
      .min(1)
      .describe("Package name for a globally installed worktree plugin module."),
    export: z
      .string()
      .min(1)
      .optional()
      .describe("Optional module export name to load. Defaults to `default`."),
  })
  .describe("Reference to a custom worktree plugin loaded from a globally installed package.")

export type WorktreePluginPackageReference = z.infer<typeof WorktreePluginPackageReference>

/** Schema for one custom worktree plugin reference declared in root config. */
export const WorktreePluginReference = z
  .discriminatedUnion("type", [WorktreePluginPathReference, WorktreePluginPackageReference])
  .describe("Custom worktree plugin reference loaded by the daemon from global config.")

export type WorktreePluginReference = z.infer<typeof WorktreePluginReference>

/** Schema for supported package managers used by daemon-managed worktree bootstrap. */
export const WorktreeBootstrapPackageManager = z
  .enum(["bun", "pnpm", "npm", "yarn"])
  .describe("Package manager command used to prepare fresh daemon-managed worktrees.")

export type WorktreeBootstrapPackageManager = z.infer<typeof WorktreeBootstrapPackageManager>

/** Schema for daemon-managed bootstrap defaults applied to fresh worktrees. */
export const WorktreeBootstrapConfig = z
  .strictObject({
    enabled: z
      .boolean()
      .optional()
      .describe("Whether daemon-managed worktree seeding and bootstrap are enabled."),
    packageManager: WorktreeBootstrapPackageManager.optional().describe(
      "Package manager command to run when bootstrapping a fresh daemon-managed worktree.",
    ),
    installArgs: z
      .array(z.string().min(1))
      .optional()
      .describe("Additional arguments appended to the selected package-manager install command."),
    seedEnabled: z
      .boolean()
      .optional()
      .describe("Whether selected untracked artifacts should be copied into fresh worktrees."),
    seedNames: z
      .array(z.string().min(1))
      .optional()
      .describe("Recursive basename allowlist used when selecting untracked seed candidates."),
    seedPaths: z
      .array(z.string().min(1))
      .optional()
      .describe("Exact repository-relative paths added to the untracked seed candidate set."),
  })
  .describe("Daemon-managed preparation settings applied to fresh worktrees.")

export type WorktreeBootstrapConfig = z.infer<typeof WorktreeBootstrapConfig>

/** Schema for persisted daemon worktree defaults loaded from JSON. */
export const WorktreesConfig = z
  .strictObject({
    defaultFolder: z
      .string()
      .min(1)
      .optional()
      .describe("Default repository-local folder name used for daemon-managed worktrees."),
    branchPrefix: z
      .string()
      .min(1)
      .optional()
      .describe("Branch path prefix joined before generated worktree branch ids."),
    bootstrap: WorktreeBootstrapConfig.optional().describe(
      "Daemon-managed preparation defaults applied to fresh worktrees.",
    ),
    plugins: z
      .array(WorktreePluginReference)
      .optional()
      .describe("Custom worktree plugins loaded from the global Goddard config only."),
  })
  .describe("Persisted worktree defaults loaded from JSON.")

export type WorktreesConfig = z.infer<typeof WorktreesConfig>

/** Persisted session-title generation defaults loaded from JSON. */
export type SessionTitlesConfig = {
  generator?: ModelConfig
}

/** Schema for persisted session-title generation defaults loaded from JSON. */
export const SessionTitlesConfig: z.ZodType<SessionTitlesConfig> = z
  .strictObject({
    generator: textModelConfigSchema
      .optional()
      .describe("Text model selection used for background session title generation."),
  })
  .describe("Persisted session title-generation defaults loaded from JSON.")

const SESSION_IDLE_SHUTDOWN_DURATION_PATTERN = /^([1-9]\d*)\s*(ms|s|m|h)$/
const SESSION_IDLE_SHUTDOWN_DURATION_UNITS = {
  ms: 1,
  s: 1000,
  m: 60 * 1000,
  h: 60 * 60 * 1000,
} as const

/** Parses one positive session idle-shutdown duration string into milliseconds. */
export function parseSessionIdleShutdownDurationMs(value: string): number | null {
  const match = SESSION_IDLE_SHUTDOWN_DURATION_PATTERN.exec(value.trim())
  if (!match) {
    return null
  }

  const amount = Number(match[1])
  const unit = match[2] as keyof typeof SESSION_IDLE_SHUTDOWN_DURATION_UNITS
  const milliseconds = amount * SESSION_IDLE_SHUTDOWN_DURATION_UNITS[unit]
  return Number.isSafeInteger(milliseconds) ? milliseconds : null
}

/** Schema for environment variables injected into daemon-managed session processes. */
export const SessionEnvValues = z
  .record(z.string().min(1), z.string())
  .describe("Fixed session environment variable values keyed by environment variable name.")

export type SessionEnvValues = z.infer<typeof SessionEnvValues>

/** Schema for controlling environment variables available to launched session agents. */
export const SessionEnvPolicyConfig = z
  .strictObject({
    inherit: z
      .boolean()
      .optional()
      .describe("Whether launched session agents inherit the daemon host environment."),
    allow: z
      .array(z.string().min(1))
      .optional()
      .describe("Environment variable names allowed in launched session agent environments."),
    block: z
      .array(z.string().min(1))
      .optional()
      .describe("Environment variable names removed from launched session agent environments."),
    set: SessionEnvValues.optional().describe(
      "Fixed environment variable values injected by global user config only.",
    ),
  })
  .describe("Environment policy applied when launching daemon-managed session agents.")

export type SessionEnvPolicyConfig = z.infer<typeof SessionEnvPolicyConfig>

/** Schema for daemon-managed session runtime policy loaded from JSON. */
export const SessionsConfig = z
  .strictObject({
    idleShutdown: z
      .string()
      .refine((value) => parseSessionIdleShutdownDurationMs(value) !== null, {
        message: "Use a positive duration like `15m`, `1h`, `30s`, or `500ms`.",
      })
      .optional()
      .describe(
        "Positive duration after which an idle reloadable session is shut down. Examples: `15m`, `1h`, `30s`, `500ms`.",
      ),
    envPolicy: SessionEnvPolicyConfig.optional().describe(
      "Environment inheritance, filtering, and fixed global injection policy for launched session agents.",
    ),
  })
  .describe("Daemon-managed session lifecycle policy loaded from JSON.")

export type SessionsConfig = z.infer<typeof SessionsConfig>

/** Schema for package-boundary discovery settings used by the launch-session flow. */
export const SubpackagesConfig = z
  .strictObject({
    manifests: z
      .array(z.string().min(1))
      .optional()
      .describe(
        "Additional manifest filenames or relative manifest paths that mark subpackage directories.",
      ),
  })
  .describe("Persisted subpackage discovery settings loaded from JSON.")

export type SubpackagesConfig = z.infer<typeof SubpackagesConfig>

/** Worktree options accepted by the daemon session API. */
export const SessionWorktreeParams = z.strictObject({
  enabled: z.boolean().optional(),
  baseBranchName: z.string().optional(),
})

export type SessionWorktreeParams = z.infer<typeof SessionWorktreeParams>

/** Response payload fragment returned after one daemon-managed session worktree fetch. */
export const SessionWorktree = z.strictObject({
  repoRoot: z.string(),
  requestedCwd: z.string(),
  effectiveCwd: z.string(),
  worktreeDir: z.string(),
  branchName: z.string(),
  mergeTargetBranch: z.string().nullable(),
  poweredBy: z.string(),
})

export type SessionWorktree = z.infer<typeof SessionWorktree>

/** Session identity fragment shared by worktree responses. */
export type SessionWorktreeIdentity = {
  id: SessionId
  acpSessionId: string
}

/** Response payload returned after one daemon-managed session worktree fetch. */
export type GetSessionWorktreeResponse = SessionWorktreeIdentity & {
  worktree: SessionWorktree | null
}

/** Request payload used to read one daemon-managed session worktree. */
export const GetSessionWorktreeRequest = SessionIdParams

export type GetSessionWorktreeRequest = z.infer<typeof GetSessionWorktreeRequest>

/** Status values returned when evaluating whether one session worktree can merge now. */
export const SessionWorktreeMergeReadinessStatus = z.enum([
  "ready",
  "missing_worktree",
  "merge_target_branch_required",
  "merge_target_branch_missing",
  "pr_scoped_session",
  "session_active",
  "worktree_dirty",
  "primary_dirty",
  "not_ahead",
  "not_fast_forward",
  "worktree_missing",
])

export type SessionWorktreeMergeReadinessStatus = z.infer<
  typeof SessionWorktreeMergeReadinessStatus
>

/** Structured readiness facts returned for one session worktree merge evaluation. */
export const SessionWorktreeMergeReadiness = z.strictObject({
  status: SessionWorktreeMergeReadinessStatus,
  mergeTargetBranch: z.string().nullable(),
  worktreeHeadOid: z.string().nullable(),
  worktreeHeadBranch: z.string().nullable(),
  targetBranchHeadOid: z.string().nullable(),
  aheadCount: z.number().int().nonnegative(),
  syncMounted: z.boolean(),
  willAutoUnmountSync: z.boolean(),
})

export type SessionWorktreeMergeReadiness = z.infer<typeof SessionWorktreeMergeReadiness>

/** Request payload used to read merge readiness for one daemon-managed session worktree. */
export const GetSessionWorktreeMergeReadinessRequest = SessionIdParams

export type GetSessionWorktreeMergeReadinessRequest = z.infer<
  typeof GetSessionWorktreeMergeReadinessRequest
>

/** Response payload returned after reading merge readiness for one session worktree. */
export type GetSessionWorktreeMergeReadinessResponse = SessionWorktreeIdentity & {
  readiness: SessionWorktreeMergeReadiness
}

/** Request payload used to update one persisted session worktree merge target branch. */
export const SetSessionWorktreeMergeTargetBranchRequest = SessionIdParams.extend({
  mergeTargetBranch: z.string().nullable(),
})

export type SetSessionWorktreeMergeTargetBranchRequest = z.infer<
  typeof SetSessionWorktreeMergeTargetBranchRequest
>

/** Response payload returned after updating one session worktree merge target branch. */
export type SetSessionWorktreeMergeTargetBranchResponse = SessionWorktreeIdentity & {
  mergeTargetBranch: string | null
  readiness: SessionWorktreeMergeReadiness
}

/** Request payload used to merge one session worktree into its persisted target branch. */
export const MergeSessionWorktreeRequest = SessionIdParams

export type MergeSessionWorktreeRequest = z.infer<typeof MergeSessionWorktreeRequest>

/** Response payload returned after one session worktree merge attempt. */
export type MergeSessionWorktreeResponse = SessionWorktreeIdentity &
  (
    | {
        merged: true
        targetBranch: string
        sourceHeadOid: string
        previousTargetHeadOid: string
        nextTargetHeadOid: string
        syncUnmounted: boolean
        warnings: string[]
      }
    | {
        merged: false
        readiness: SessionWorktreeMergeReadiness
        syncUnmounted: boolean
        warnings: string[]
      }
  )
