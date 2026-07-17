import { $type, defineIpcRoutes, http, metadata } from "@goddard-ai/ipc"

import {
  CancelSessionRequest,
  CompleteSessionRequest,
  CreateSessionRequest,
  DeclareSessionInitiativeRequest,
  GetSessionChangesRequest as GetSessionChangesRequestSchema,
  GetSessionHistoryRequest as GetSessionHistoryRequestSchema,
  GetSessionWorktreeMergeReadinessRequest,
  GetSessionWorktreeRequest,
  ListSessionProfilesRequest,
  ListSessionsRequest,
  MergeSessionWorktreeRequest,
  PrepareSessionLaunchWorktreeRequest,
  ReleaseSessionLaunchLeaseRequest,
  ReleaseSessionLaunchWorktreeRequest,
  RemoveSessionProfileRequest,
  ReportSessionBlockerRequest,
  ReportSessionTurnEndedRequest,
  ResolveSessionTokenRequest,
  SendSessionMessageRequest,
  SessionComposerSuggestionsRequest,
  SessionDraftSuggestionsRequest,
  SessionIdParams,
  SessionLaunchPreviewRequest,
  SessionSubpackagesRequest,
  SetSessionConfigOptionRequest,
  SetSessionModelRequest,
  SetSessionWorktreeMergeTargetBranchRequest,
  SetSessionProfileRequest,
  SteerSessionRequest,
  type CancelSessionResponse,
  type CompleteSessionResponse,
  type CreateSessionResponse,
  type GetSessionChangesResponse,
  type GetSessionDiagnosticsResponse,
  type GetSessionHistoryResponse,
  type GetSessionResponse,
  type GetSessionWorktreeMergeReadinessResponse,
  type GetSessionWorktreeResponse,
  type ListSessionsResponse,
  type MergeSessionWorktreeResponse,
  type PopQueuedSessionPromptResponse,
  type PrepareSessionLaunchWorktreeResponse,
  type ReleaseSessionLaunchLeaseResponse,
  type ReleaseSessionLaunchWorktreeResponse,
  type ReportSessionResponse,
  type SessionComposerSuggestionsResponse,
  type SessionLaunchPreviewResponse,
  type SessionProfilesResponse,
  type SessionSubpackagesResponse,
  type SetSessionConfigOptionResponse,
  type SetSessionModelResponse,
  type SetSessionWorktreeMergeTargetBranchResponse,
  type ShutdownSessionResponse,
  type SteerSessionResponse,
} from "./schema.ts"

export const sessionIpcRoutes = defineIpcRoutes({
  session: http.resource("session", {
    ...metadata({
      description: "Session control.",
    }),
    /** Creates one session record. */
    create: http.post("create", {
      ...metadata({
        description: "Creates one session record.",
      }),
      body: CreateSessionRequest,
      response: $type<CreateSessionResponse>(),
    }),
    /** Lists sessions and pagination state. */
    list: http.post("list", {
      ...metadata({
        description: "Lists sessions and pagination state.",
      }),
      body: ListSessionsRequest,
      response: $type<ListSessionsResponse>(),
    }),
    /** Fetches one session record. */
    get: http.post("get", {
      ...metadata({
        description: "Fetches one session record.",
      }),
      body: SessionIdParams,
      response: $type<GetSessionResponse>(),
    }),
    /** Reconnects to one session record. */
    connect: http.post("connect", {
      ...metadata({
        description: "Reconnects to one session record.",
      }),
      body: SessionIdParams,
      response: $type<GetSessionResponse>(),
    }),
    /** Reads one session history with session identity and connection state. */
    history: http.post("history", {
      ...metadata({
        description: "Reads one session history with session identity and connection state.",
      }),
      body: GetSessionHistoryRequestSchema,
      response: $type<GetSessionHistoryResponse>(),
    }),
    /** Reads the current git diff for one session workspace. */
    changes: http.post("changes", {
      ...metadata({
        description: "Reads the current git diff for one session workspace.",
      }),
      body: GetSessionChangesRequestSchema,
      response: $type<GetSessionChangesResponse>(),
    }),
    /** Reads session-scoped composer suggestions for one chat trigger and filter query. */
    composerSuggestions: http.post("composer-suggestions", {
      ...metadata({
        description:
          "Reads session-scoped composer suggestions for one chat trigger and filter query.",
      }),
      body: SessionComposerSuggestionsRequest,
      response: $type<SessionComposerSuggestionsResponse>(),
    }),
    /** Reads draft composer suggestions that only depend on one repository cwd. */
    draftSuggestions: http.post("draft-suggestions", {
      ...metadata({
        description: "Reads draft composer suggestions that only depend on one repository cwd.",
      }),
      body: SessionDraftSuggestionsRequest,
      response: $type<SessionComposerSuggestionsResponse>(),
    }),
    /** Loads launch-time adapter and repository capabilities before a session is created. */
    launchPreview: http.post("launch-preview", {
      ...metadata({
        description:
          "Loads launch-time adapter and repository capabilities before a session is created.",
      }),
      body: SessionLaunchPreviewRequest,
      response: $type<SessionLaunchPreviewResponse>(),
    }),
    launchLease: http.resource("launch-lease", {
      ...metadata({
        description: "Session launch-lease cleanup control.",
      }),
      /** Schedules one abandoned launch lease for delayed release. */
      release: http.post("release", {
        ...metadata({
          description: "Schedules one abandoned launch lease for delayed release.",
        }),
        body: ReleaseSessionLaunchLeaseRequest,
        response: $type<ReleaseSessionLaunchLeaseResponse>(),
      }),
    }),
    launchWorktree: http.resource("launch-worktree", {
      ...metadata({
        description: "Session launch-dialog worktree preparation and cleanup.",
      }),
      /** Prepares one launch-dialog worktree for possible session creation. */
      prepare: http.post("prepare", {
        ...metadata({
          description: "Prepares one launch-dialog worktree for possible session creation.",
        }),
        body: PrepareSessionLaunchWorktreeRequest,
        response: $type<PrepareSessionLaunchWorktreeResponse>(),
      }),
      /** Schedules one abandoned launch-dialog worktree for delayed cleanup. */
      release: http.post("release", {
        ...metadata({
          description: "Schedules one abandoned launch-dialog worktree for delayed cleanup.",
        }),
        body: ReleaseSessionLaunchWorktreeRequest,
        response: $type<ReleaseSessionLaunchWorktreeResponse>(),
      }),
    }),
    /** Discovers launchable subpackage working directories under one project cwd. */
    subpackages: http.post("subpackages", {
      ...metadata({
        description: "Discovers launchable subpackage working directories under one project cwd.",
      }),
      body: SessionSubpackagesRequest,
      response: $type<SessionSubpackagesResponse>(),
    }),
    /** Reads one session diagnostics with event history and connection state. */
    diagnostics: http.post("diagnostics", {
      ...metadata({
        description: "Reads one session diagnostics with event history and connection state.",
      }),
      body: SessionIdParams,
      response: $type<GetSessionDiagnosticsResponse>(),
    }),
    worktree: http.resource("worktree", {
      ...metadata({
        description: "Session worktree metadata.",
      }),
      /** Reads persisted worktree metadata attached to one session. */
      get: http.post("get", {
        ...metadata({
          description: "Reads persisted worktree metadata attached to one session.",
        }),
        body: GetSessionWorktreeRequest,
        response: $type<GetSessionWorktreeResponse>(),
      }),
      /** Reads merge readiness for one session worktree. */
      mergeReadiness: http.post("merge-readiness", {
        ...metadata({
          description: "Reads merge readiness for one session worktree.",
        }),
        body: GetSessionWorktreeMergeReadinessRequest,
        response: $type<GetSessionWorktreeMergeReadinessResponse>(),
      }),
      mergeTargetBranch: http.resource("merge-target-branch", {
        ...metadata({
          description: "Session worktree merge-target control.",
        }),
        /** Updates the persisted merge target branch for one session worktree. */
        set: http.post("set", {
          ...metadata({
            description: "Updates the persisted merge target branch for one session worktree.",
          }),
          body: SetSessionWorktreeMergeTargetBranchRequest,
          response: $type<SetSessionWorktreeMergeTargetBranchResponse>(),
        }),
      }),
      /** Merges one session worktree into its persisted target branch. */
      merge: http.post("merge", {
        ...metadata({
          description: "Merges one session worktree into its persisted target branch.",
        }),
        body: MergeSessionWorktreeRequest,
        response: $type<MergeSessionWorktreeResponse>(),
      }),
    }),
    /** Shuts down one session and reports whether shutdown succeeded. */
    shutdown: http.post("shutdown", {
      ...metadata({
        description: "Shuts down one session and reports whether shutdown succeeded.",
      }),
      body: SessionIdParams,
      response: $type<ShutdownSessionResponse>(),
    }),
    /** Cancels the active turn and returns any queued prompts aborted instead of replaying. */
    cancel: http.post("cancel", {
      ...metadata({
        description:
          "Cancels the active turn and returns any queued prompts aborted instead of replaying.",
      }),
      body: CancelSessionRequest,
      response: $type<CancelSessionResponse>(),
    }),
    /** Cancels the active turn and injects one replacement prompt after a safe boundary. */
    steer: http.post("steer", {
      ...metadata({
        description:
          "Cancels the active turn and injects one replacement prompt after a safe boundary.",
      }),
      body: SteerSessionRequest,
      response: $type<SteerSessionResponse>(),
    }),
    /** Removes the newest client-originated queued prompt before dispatch and returns it. */
    popQueuedPrompt: http.post("pop-queued-prompt", {
      ...metadata({
        description:
          "Removes the newest client-originated queued prompt before dispatch and returns it.",
      }),
      body: SessionIdParams,
      response: $type<PopQueuedSessionPromptResponse>(),
    }),
    /** Sends one raw message to a session and reports whether it was accepted. */
    send: http.post("send", {
      ...metadata({
        description: "Sends one raw message to a session and reports whether it was accepted.",
      }),
      body: SendSessionMessageRequest,
      response: $type<{ accepted: true }>(),
    }),
    configOption: http.resource("config-option", {
      ...metadata({
        description: "Active session ACP config-option control.",
      }),
      /** Updates one ACP config option on an active session. */
      set: http.post("set", {
        ...metadata({
          description: "Updates one ACP config option on an active session.",
        }),
        body: SetSessionConfigOptionRequest,
        response: $type<SetSessionConfigOptionResponse>(),
      }),
    }),
    model: http.resource("model", {
      ...metadata({
        description: "Active session ACP model control.",
      }),
      /** Updates the ACP model on an active session. */
      set: http.post("set", {
        ...metadata({
          description: "Updates the ACP model on an active session.",
        }),
        body: SetSessionModelRequest,
        response: $type<SetSessionModelResponse>(),
      }),
    }),
    profile: http.resource("profile", {
      ...metadata({
        description: "Global session profile management.",
      }),
      /** Lists globally configured session profiles. */
      list: http.post("list", {
        ...metadata({
          description: "Lists globally configured session profiles.",
        }),
        body: ListSessionProfilesRequest,
        response: $type<SessionProfilesResponse>(),
      }),
      /** Replaces one fixed session profile for an agent harness. */
      set: http.post("set", {
        ...metadata({
          description: "Replaces one fixed session profile for an agent harness.",
        }),
        body: SetSessionProfileRequest,
        response: $type<SessionProfilesResponse>(),
      }),
      /** Removes one fixed session profile from an agent harness. */
      remove: http.post("remove", {
        ...metadata({
          description: "Removes one fixed session profile from an agent harness.",
        }),
        body: RemoveSessionProfileRequest,
        response: $type<SessionProfilesResponse>(),
      }),
    }),
    /** Marks one session inbox row completed without shutting down the session. */
    complete: http.post("complete", {
      ...metadata({
        description: "Marks one session inbox row completed without shutting down the session.",
      }),
      body: CompleteSessionRequest,
      response: $type<CompleteSessionResponse>(),
    }),
    /** Records the current session initiative without creating an inbox row. */
    declareInitiative: http.post("declare-initiative", {
      ...metadata({
        description: "Records the current session initiative without creating an inbox row.",
      }),
      body: DeclareSessionInitiativeRequest,
      response: $type<ReportSessionResponse>(),
    }),
    /** Reports a session blocker and marks the session inbox row unread. */
    reportBlocker: http.post("report-blocker", {
      ...metadata({
        description: "Reports a session blocker and marks the session inbox row unread.",
      }),
      body: ReportSessionBlockerRequest,
      response: $type<ReportSessionResponse>(),
    }),
    /** Reports an end-of-turn session update when no other entity claimed attention. */
    reportTurnEnded: http.post("report-turn-ended", {
      ...metadata({
        description:
          "Reports an end-of-turn session update when no other entity claimed attention.",
      }),
      body: ReportSessionTurnEndedRequest,
      response: $type<ReportSessionResponse>(),
    }),
    /** Resolves one session token to its session id. */
    resolveToken: http.post("resolve-token", {
      ...metadata({
        description: "Resolves one session token to its session id.",
      }),
      body: ResolveSessionTokenRequest,
      response: $type<{ id: string }>(),
    }),
  }),
})
