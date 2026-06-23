import { $type, defineIpcRoutes, http, ipcMetadata, ndjson } from "@goddard-ai/ipc"

import {
  CancelSessionRequest,
  CompleteSessionRequest,
  CreateSessionRequest,
  DeclareSessionInitiativeRequest,
  GetSessionChangesRequest as GetSessionChangesRequestSchema,
  GetSessionHistoryRequest as GetSessionHistoryRequestSchema,
  GetSessionWorktreeRequest,
  ListSessionsRequest,
  PrepareSessionLaunchWorktreeRequest,
  ReleaseSessionLaunchLeaseRequest,
  ReleaseSessionLaunchWorktreeRequest,
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
  SteerSessionRequest,
  type CancelSessionResponse,
  type CompleteSessionResponse,
  type CreateSessionResponse,
  type GetSessionChangesResponse,
  type GetSessionDiagnosticsResponse,
  type GetSessionHistoryResponse,
  type GetSessionResponse,
  type GetSessionWorktreeResponse,
  type ListSessionsResponse,
  type PopQueuedSessionPromptResponse,
  type PrepareSessionLaunchWorktreeResponse,
  type ReleaseSessionLaunchLeaseResponse,
  type ReleaseSessionLaunchWorktreeResponse,
  type ReportSessionResponse,
  type SessionComposerSuggestionsResponse,
  type SessionLaunchPreviewResponse,
  type SessionSubpackagesResponse,
  type SetSessionConfigOptionResponse,
  type SetSessionModelResponse,
  type ShutdownSessionResponse,
  type SteerSessionResponse,
} from "./schema.ts"

export const sessionIpcRoutes = defineIpcRoutes({
  session: http.resource("session", {
    ...ipcMetadata({
      description: "Daemon-managed session control.",
    }),
    /** Creates one daemon-managed session record. */
    create: http.post("create", {
      ...ipcMetadata({
        description: "Creates one daemon-managed session record.",
      }),
      body: CreateSessionRequest,
      response: $type<CreateSessionResponse>(),
    }),
    /** Lists daemon-managed sessions and pagination state. */
    list: http.post("list", {
      ...ipcMetadata({
        description: "Lists daemon-managed sessions and pagination state.",
      }),
      body: ListSessionsRequest,
      response: $type<ListSessionsResponse>(),
    }),
    /** Fetches one daemon-managed session record. */
    get: http.post("get", {
      ...ipcMetadata({
        description: "Fetches one daemon-managed session record.",
      }),
      body: SessionIdParams,
      response: $type<GetSessionResponse>(),
    }),
    /** Reconnects to one daemon-managed session record. */
    connect: http.post("connect", {
      ...ipcMetadata({
        description: "Reconnects to one daemon-managed session record.",
      }),
      body: SessionIdParams,
      response: $type<GetSessionResponse>(),
    }),
    /** Reads one daemon-managed session history with session identity and connection state. */
    history: http.post("history", {
      ...ipcMetadata({
        description:
          "Reads one daemon-managed session history with session identity and connection state.",
      }),
      body: GetSessionHistoryRequestSchema,
      response: $type<GetSessionHistoryResponse>(),
    }),
    /** Reads the current git diff for one daemon-managed session workspace. */
    changes: http.post("changes", {
      ...ipcMetadata({
        description: "Reads the current git diff for one daemon-managed session workspace.",
      }),
      body: GetSessionChangesRequestSchema,
      response: $type<GetSessionChangesResponse>(),
    }),
    /** Reads session-scoped composer suggestions for one chat trigger and filter query. */
    composerSuggestions: http.post("composer-suggestions", {
      ...ipcMetadata({
        description:
          "Reads session-scoped composer suggestions for one chat trigger and filter query.",
      }),
      body: SessionComposerSuggestionsRequest,
      response: $type<SessionComposerSuggestionsResponse>(),
    }),
    /** Reads draft composer suggestions that only depend on one repository cwd. */
    draftSuggestions: http.post("draft-suggestions", {
      ...ipcMetadata({
        description: "Reads draft composer suggestions that only depend on one repository cwd.",
      }),
      body: SessionDraftSuggestionsRequest,
      response: $type<SessionComposerSuggestionsResponse>(),
    }),
    /** Loads launch-time adapter and repository capabilities before a session is created. */
    launchPreview: http.post("launch-preview", {
      ...ipcMetadata({
        description:
          "Loads launch-time adapter and repository capabilities before a session is created.",
      }),
      body: SessionLaunchPreviewRequest,
      response: $type<SessionLaunchPreviewResponse>(),
    }),
    launchLease: http.resource("launch-lease", {
      ...ipcMetadata({
        description: "Session launch-lease cleanup control.",
      }),
      /** Schedules one abandoned launch lease for delayed release. */
      release: http.post("release", {
        ...ipcMetadata({
          description: "Schedules one abandoned launch lease for delayed release.",
        }),
        body: ReleaseSessionLaunchLeaseRequest,
        response: $type<ReleaseSessionLaunchLeaseResponse>(),
      }),
    }),
    launchWorktree: http.resource("launch-worktree", {
      ...ipcMetadata({
        description: "Session launch-dialog worktree preparation and cleanup.",
      }),
      /** Prepares one launch-dialog worktree for possible session creation. */
      prepare: http.post("prepare", {
        ...ipcMetadata({
          description: "Prepares one launch-dialog worktree for possible session creation.",
        }),
        body: PrepareSessionLaunchWorktreeRequest,
        response: $type<PrepareSessionLaunchWorktreeResponse>(),
      }),
      /** Schedules one abandoned launch-dialog worktree for delayed cleanup. */
      release: http.post("release", {
        ...ipcMetadata({
          description: "Schedules one abandoned launch-dialog worktree for delayed cleanup.",
        }),
        body: ReleaseSessionLaunchWorktreeRequest,
        response: $type<ReleaseSessionLaunchWorktreeResponse>(),
      }),
    }),
    /** Discovers launchable subpackage working directories under one project cwd. */
    subpackages: http.post("subpackages", {
      ...ipcMetadata({
        description: "Discovers launchable subpackage working directories under one project cwd.",
      }),
      body: SessionSubpackagesRequest,
      response: $type<SessionSubpackagesResponse>(),
    }),
    /** Reads one daemon-managed session diagnostics with event history and connection state. */
    diagnostics: http.post("diagnostics", {
      ...ipcMetadata({
        description:
          "Reads one daemon-managed session diagnostics with event history and connection state.",
      }),
      body: SessionIdParams,
      response: $type<GetSessionDiagnosticsResponse>(),
    }),
    worktree: http.resource("worktree", {
      ...ipcMetadata({
        description: "Daemon-managed session worktree metadata.",
      }),
      /** Reads persisted worktree metadata attached to one daemon-managed session. */
      get: http.post("get", {
        ...ipcMetadata({
          description: "Reads persisted worktree metadata attached to one daemon-managed session.",
        }),
        body: GetSessionWorktreeRequest,
        response: $type<GetSessionWorktreeResponse>(),
      }),
    }),
    /** Shuts down one daemon-managed session and reports whether shutdown succeeded. */
    shutdown: http.post("shutdown", {
      ...ipcMetadata({
        description:
          "Shuts down one daemon-managed session and reports whether shutdown succeeded.",
      }),
      body: SessionIdParams,
      response: $type<ShutdownSessionResponse>(),
    }),
    /** Cancels the active turn and returns any queued prompts the daemon aborted instead of replaying. */
    cancel: http.post("cancel", {
      ...ipcMetadata({
        description:
          "Cancels the active turn and returns any queued prompts the daemon aborted instead of replaying.",
      }),
      body: CancelSessionRequest,
      response: $type<CancelSessionResponse>(),
    }),
    /** Cancels the active turn and injects one replacement prompt after the daemon observes a safe boundary. */
    steer: http.post("steer", {
      ...ipcMetadata({
        description:
          "Cancels the active turn and injects one replacement prompt after the daemon observes a safe boundary.",
      }),
      body: SteerSessionRequest,
      response: $type<SteerSessionResponse>(),
    }),
    /** Removes the newest client-originated queued prompt before dispatch and returns it. */
    popQueuedPrompt: http.post("pop-queued-prompt", {
      ...ipcMetadata({
        description:
          "Removes the newest client-originated queued prompt before dispatch and returns it.",
      }),
      body: SessionIdParams,
      response: $type<PopQueuedSessionPromptResponse>(),
    }),
    /** Sends one raw message to a daemon-managed session and reports whether it was accepted. */
    send: http.post("send", {
      ...ipcMetadata({
        description:
          "Sends one raw message to a daemon-managed session and reports whether it was accepted.",
      }),
      body: SendSessionMessageRequest,
      response: $type<{ accepted: true }>(),
    }),
    configOption: http.resource("config-option", {
      ...ipcMetadata({
        description: "Active session ACP config-option control.",
      }),
      /** Updates one ACP config option on an active daemon-managed session. */
      set: http.post("set", {
        ...ipcMetadata({
          description: "Updates one ACP config option on an active daemon-managed session.",
        }),
        body: SetSessionConfigOptionRequest,
        response: $type<SetSessionConfigOptionResponse>(),
      }),
    }),
    model: http.resource("model", {
      ...ipcMetadata({
        description: "Active session ACP model control.",
      }),
      /** Updates the ACP model on an active daemon-managed session. */
      set: http.post("set", {
        ...ipcMetadata({
          description: "Updates the ACP model on an active daemon-managed session.",
        }),
        body: SetSessionModelRequest,
        response: $type<SetSessionModelResponse>(),
      }),
    }),
    /** Marks one session inbox row completed without shutting down the session. */
    complete: http.post("complete", {
      ...ipcMetadata({
        description: "Marks one session inbox row completed without shutting down the session.",
      }),
      body: CompleteSessionRequest,
      response: $type<CompleteSessionResponse>(),
    }),
    /** Records the current session initiative without creating an inbox row. */
    declareInitiative: http.post("declare-initiative", {
      ...ipcMetadata({
        description: "Records the current session initiative without creating an inbox row.",
      }),
      body: DeclareSessionInitiativeRequest,
      response: $type<ReportSessionResponse>(),
    }),
    /** Reports a session blocker and marks the session inbox row unread. */
    reportBlocker: http.post("report-blocker", {
      ...ipcMetadata({
        description: "Reports a session blocker and marks the session inbox row unread.",
      }),
      body: ReportSessionBlockerRequest,
      response: $type<ReportSessionResponse>(),
    }),
    /** Reports an end-of-turn session update when no other entity claimed attention. */
    reportTurnEnded: http.post("report-turn-ended", {
      ...ipcMetadata({
        description:
          "Reports an end-of-turn session update when no other entity claimed attention.",
      }),
      body: ReportSessionTurnEndedRequest,
      response: $type<ReportSessionResponse>(),
    }),
    /** Resolves one daemon session token to its daemon session id. */
    resolveToken: http.post("resolve-token", {
      ...ipcMetadata({
        description: "Resolves one daemon session token to its daemon session id.",
      }),
      body: ResolveSessionTokenRequest,
      response: $type<{ id: string }>(),
    }),
    /** Streams live daemon-published ACP messages for one daemon-managed session id. */
    streamMessages: http.get("stream-messages", {
      ...ipcMetadata({
        description:
          "Streams live daemon-published ACP messages for one daemon-managed session id.",
      }),
      query: SessionIdParams,
      response: ndjson.$type<SessionMessageEvent>(),
    }),
    /** Streams app-wide daemon session lifecycle updates without observing transcript messages. */
    streamLifecycle: http.get("stream-lifecycle", {
      ...ipcMetadata({
        description:
          "Streams app-wide daemon session lifecycle updates without observing transcript messages.",
      }),
      response: ndjson.$type<SessionLifecycleEvent>(),
    }),
  }),
})
