import * as acp from "@agentclientprotocol/sdk"
import { z } from "zod"

import { ACPAdapterName } from "../acp-adapters.ts"
import { AgentDistribution } from "../agent-distribution.ts"
import { DaemonSessionId, DaemonSessionIdParams } from "../common/params.ts"
import {
  DaemonSessionMetadata,
  DaemonSessionStopReason,
  DaemonSessionTurnCompletionKind,
  DaemonSessionTurnPromptRequestId,
  type DaemonSession,
  type DaemonSessionDiagnosticEvent,
  type DaemonWorkforce,
} from "./store.ts"

/** Session-start initial prompt values accepted by the daemon session API. */
export const InitialPromptOption = z.union([z.string(), z.array(z.custom<acp.ContentBlock>())])

export type InitialPromptOption = z.infer<typeof InitialPromptOption>

/** Worktree options accepted by the daemon session API. */
export const SessionWorktreeSyncParams = z.strictObject({
  enabled: z.boolean().optional(),
})

export type SessionWorktreeSyncParams = z.infer<typeof SessionWorktreeSyncParams>

/** Worktree options accepted by the daemon session API. */
export const SessionWorktreeParams = z.strictObject({
  enabled: z.boolean().optional(),
  baseBranchName: z.string().optional(),
  sync: SessionWorktreeSyncParams.optional(),
})

export type SessionWorktreeParams = z.infer<typeof SessionWorktreeParams>

/** Workforce attachment stored separately from the base daemon session record. */
export const SessionWorkforceParams = z.strictObject({
  rootDir: z.string().optional(),
  agentId: z.string().optional(),
  requestId: z.string().optional(),
})

export type SessionWorkforceParams = z.infer<typeof SessionWorkforceParams>

/** Live sync-session state merged into one daemon worktree response. */
export const SessionWorktreeSyncState = z.strictObject({
  status: z.literal("mounted"),
  conflictPreference: z.literal("worktree"),
  baseOid: z.string(),
  primaryLatestSnapshotOid: z.string().nullable(),
  worktreeLatestSnapshotOid: z.string().nullable(),
  resultSnapshotOid: z.string().nullable(),
  primaryRecoverySnapshotOid: z.string().nullable(),
  lastSyncAt: z.number().int().nullable(),
})

export type SessionWorktreeSyncState = z.infer<typeof SessionWorktreeSyncState>

/** Response payload fragment returned after one daemon-managed session worktree fetch. */
export const SessionWorktree = z.strictObject({
  repoRoot: z.string(),
  requestedCwd: z.string(),
  effectiveCwd: z.string(),
  worktreeDir: z.string(),
  branchName: z.string(),
  poweredBy: z.string(),
  sync: SessionWorktreeSyncState.nullable(),
})

export type SessionWorktree = z.infer<typeof SessionWorktree>

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
  agent: z.union([z.string() as z.ZodType<ACPAdapterName>, AgentDistribution]).optional(),
  cwd: z.string(),
  worktree: SessionWorktreeParams.optional(),
  workforce: SessionWorkforceParams.optional(),
  mcpServers: z.array(z.custom<acp.McpServer>()),
  systemPrompt: z.string(),
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
export const SessionPathParams = DaemonSessionIdParams

export type SessionPathParams = z.infer<typeof SessionPathParams>

/** Request payload used to read one page of daemon-managed session turn history. */
export const GetSessionHistoryRequest = DaemonSessionIdParams.extend({
  cursor: z.string().optional(),
  limit: z.number().int().positive().optional(),
})

export type GetSessionHistoryRequest = z.infer<typeof GetSessionHistoryRequest>

/** Request payload used to read the current git diff for one daemon-managed session workspace. */
export const GetSessionChangesRequest = DaemonSessionIdParams

export type GetSessionChangesRequest = z.infer<typeof GetSessionChangesRequest>

/** Trigger categories supported by the session chat composer suggestion API. */
export const SessionComposerSuggestionTrigger = z.enum(["at", "dollar", "slash"])

export type SessionComposerSuggestionTrigger = z.infer<typeof SessionComposerSuggestionTrigger>

/** Trigger categories supported by the launch dialog composer suggestion API. */
export const SessionDraftSuggestionTrigger = z.enum(["at", "dollar"])

export type SessionDraftSuggestionTrigger = z.infer<typeof SessionDraftSuggestionTrigger>

/** Request payload used to read session-scoped composer suggestions for one trigger. */
export const SessionComposerSuggestionsRequest = DaemonSessionIdParams.extend({
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

/** Filesystem-backed suggestion item returned for one `@` trigger lookup. */
export const SessionComposerFileSuggestion = z.strictObject({
  type: z.union([z.literal("file"), z.literal("folder")]),
  path: z.string(),
  uri: z.string(),
  label: z.string(),
  detail: z.string(),
})

export type SessionComposerFileSuggestion = z.infer<typeof SessionComposerFileSuggestion>

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
  SessionComposerFileSuggestion,
  SessionComposerSkillSuggestion,
  SessionComposerSlashCommandSuggestion,
])

export type SessionComposerSuggestion = z.infer<typeof SessionComposerSuggestion>

/** Response payload returned after reading session-scoped composer suggestions. */
export const SessionComposerSuggestionsResponse = z.strictObject({
  suggestions: z.array(SessionComposerSuggestion),
})

export type SessionComposerSuggestionsResponse = z.infer<typeof SessionComposerSuggestionsResponse>

/** One selectable git branch returned for the launch-session flow. */
export const SessionLaunchBranch = z.strictObject({
  name: z.string(),
  current: z.boolean(),
})

export type SessionLaunchBranch = z.infer<typeof SessionLaunchBranch>

/** Request payload used to inspect launch-time adapter and repository capabilities. */
export const SessionLaunchPreviewRequest = z.strictObject({
  agent: z.union([z.string() as z.ZodType<ACPAdapterName>, AgentDistribution]),
  cwd: z.string(),
})

export type SessionLaunchPreviewRequest = z.infer<typeof SessionLaunchPreviewRequest>

/** Response payload returned after loading launch-time adapter and repository capabilities. */
export type SessionLaunchPreviewResponse = {
  repoRoot: string | null
  branches: SessionLaunchBranch[]
  models: acp.SessionModelState | null
  configOptions: acp.SessionConfigOption[]
  slashCommands: SessionComposerSlashCommandSuggestion[]
}

/** JSON-RPC request ids surfaced for queued and aborted prompt bookkeeping. */
export const SessionPromptId = z.union([z.string(), z.number()])

export type SessionPromptId = z.infer<typeof SessionPromptId>

/** Request payload used to forward one raw ACP message to a daemon-managed session. */
export const SendSessionMessageRequest = z.strictObject({
  id: DaemonSessionId,
  message: z.unknown(),
})

/** Compile-time shape of one raw ACP message forwarded to a daemon-managed session. */
export interface SendSessionMessageRequest {
  id: DaemonSessionId
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

/** Request payload used to cancel the active turn for one daemon-managed session. */
export const CancelSessionRequest = z.strictObject({
  id: DaemonSessionId,
})

export type CancelSessionRequest = z.infer<typeof CancelSessionRequest>

/** Request payload used to cancel the active turn and replace it with one new prompt. */
export const SteerSessionRequest = DaemonSessionIdParams.extend({
  prompt: InitialPromptOption,
})

export type SteerSessionRequest = z.infer<typeof SteerSessionRequest>

/** Stream payload emitted for one daemon-managed ACP session message. */
export const SessionMessageEvent = z.strictObject({
  id: DaemonSessionId,
  message: z.unknown(),
})

/** Compile-time shape of one daemon-managed ACP session message event. */
export interface SessionMessageEvent {
  id: DaemonSessionId
  message: acp.AnyMessage
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
  sessionId: DaemonSessionId
}

/** Stable identity values used to address one daemon-managed session. */
export type SessionIdentity = {
  id: DaemonSessionId
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
  messages: z.custom<acp.AnyMessage[]>(),
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
  messages: acp.AnyMessage[]
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

/** Response payload returned after one daemon-managed session worktree fetch. */
export type GetSessionWorktreeResponse = SessionIdentity & {
  worktree: SessionWorktree | null
}

/** Request payload used to mount sync on one daemon-managed session worktree. */
export const MountSessionWorktreeSyncRequest = DaemonSessionIdParams

export type MountSessionWorktreeSyncRequest = z.infer<typeof MountSessionWorktreeSyncRequest>

/** Request payload used to ask the daemon to run its normal worktree sync cycle immediately. */
export const SyncSessionWorktreeRequest = DaemonSessionIdParams

export type SyncSessionWorktreeRequest = z.infer<typeof SyncSessionWorktreeRequest>

/** Request payload used to unmount sync from one daemon-managed session worktree. */
export const UnmountSessionWorktreeSyncRequest = DaemonSessionIdParams

export type UnmountSessionWorktreeSyncRequest = z.infer<typeof UnmountSessionWorktreeSyncRequest>

/** Response payload returned after one daemon-managed worktree sync mutation. */
export type MutateSessionWorktreeResponse = SessionIdentity & {
  worktree: SessionWorktree | null
  warnings: string[]
}

/** Response payload returned after one daemon-managed session workforce fetch. */
export type GetSessionWorkforceResponse = SessionIdentity & {
  workforce: DaemonWorkforce | null
}

/** Response payload returned after one daemon-managed session shutdown request. */
export type ShutdownSessionResponse = {
  id: DaemonSessionId
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
