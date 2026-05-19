import { $type, defineIpcRoutes, http, ndjson } from "@goddard-ai/ipc"

import {
  CancelSessionRequest,
  CompleteSessionRequest,
  CreateSessionRequest,
  DeclareSessionInitiativeRequest,
  GetSessionChangesRequest as GetSessionChangesRequestSchema,
  GetSessionHistoryRequest as GetSessionHistoryRequestSchema,
  GetSessionWorktreeRequest,
  ListSessionsRequest,
  ReleaseSessionLaunchLeaseRequest,
  ReportSessionBlockerRequest,
  ReportSessionTurnEndedRequest,
  ResolveSessionTokenRequest,
  SendSessionMessageRequest,
  SessionComposerSuggestionsRequest,
  SessionDraftSuggestionsRequest,
  SessionIdParams,
  SessionLaunchPreviewRequest,
  SessionLifecycleEvent,
  SessionMessageEvent,
  SessionSubpackagesRequest,
  SetSessionConfigOptionRequest,
  SetSessionModelRequest,
  SteerSessionRequest,
  TranscribeAudioRequest,
  type CancelSessionResponse,
  type CompleteSessionResponse,
  type CreateSessionResponse,
  type GetSessionChangesResponse,
  type GetSessionDiagnosticsResponse,
  type GetSessionHistoryResponse,
  type GetSessionResponse,
  type GetSessionWorktreeResponse,
  type ListSessionsResponse,
  type ReleaseSessionLaunchLeaseResponse,
  type ReportSessionResponse,
  type SessionComposerSuggestionsResponse,
  type SessionLaunchPreviewResponse,
  type SessionSubpackagesResponse,
  type SetSessionConfigOptionResponse,
  type SetSessionModelResponse,
  type ShutdownSessionResponse,
  type SteerSessionResponse,
  type TranscribeAudioResponse,
} from "./schema.ts"

export const sessionIpcRoutes = defineIpcRoutes({
  transcription: http.resource("transcription", {
    /** Transcribes one audio payload through the daemon's configured global transcription model. */
    transcribe: http.post("transcribe", {
      body: TranscribeAudioRequest,
      response: $type<TranscribeAudioResponse>(),
    }),
  }),
  session: http.resource("session", {
    /** Creates one daemon-managed session record. */
    create: http.post("create", {
      body: CreateSessionRequest,
      response: $type<CreateSessionResponse>(),
    }),
    /** Lists daemon-managed sessions and pagination state. */
    list: http.post("list", {
      body: ListSessionsRequest,
      response: $type<ListSessionsResponse>(),
    }),
    /** Fetches one daemon-managed session record. */
    get: http.post("get", {
      body: SessionIdParams,
      response: $type<GetSessionResponse>(),
    }),
    /** Reconnects to one daemon-managed session record. */
    connect: http.post("connect", {
      body: SessionIdParams,
      response: $type<GetSessionResponse>(),
    }),
    /** Reads one daemon-managed session history with session identity and connection state. */
    history: http.post("history", {
      body: GetSessionHistoryRequestSchema,
      response: $type<GetSessionHistoryResponse>(),
    }),
    /** Reads the current git diff for one daemon-managed session workspace. */
    changes: http.post("changes", {
      body: GetSessionChangesRequestSchema,
      response: $type<GetSessionChangesResponse>(),
    }),
    /** Reads session-scoped composer suggestions for one chat trigger and filter query. */
    composerSuggestions: http.post("composer-suggestions", {
      body: SessionComposerSuggestionsRequest,
      response: $type<SessionComposerSuggestionsResponse>(),
    }),
    /** Reads draft composer suggestions that only depend on one repository cwd. */
    draftSuggestions: http.post("draft-suggestions", {
      body: SessionDraftSuggestionsRequest,
      response: $type<SessionComposerSuggestionsResponse>(),
    }),
    /** Loads launch-time adapter and repository capabilities before a session is created. */
    launchPreview: http.post("launch-preview", {
      body: SessionLaunchPreviewRequest,
      response: $type<SessionLaunchPreviewResponse>(),
    }),
    launchLease: http.resource("launch-lease", {
      /** Schedules one abandoned launch lease for delayed release. */
      release: http.post("release", {
        body: ReleaseSessionLaunchLeaseRequest,
        response: $type<ReleaseSessionLaunchLeaseResponse>(),
      }),
    }),
    /** Discovers launchable subpackage working directories under one project cwd. */
    subpackages: http.post("subpackages", {
      body: SessionSubpackagesRequest,
      response: $type<SessionSubpackagesResponse>(),
    }),
    /** Reads one daemon-managed session diagnostics with event history and connection state. */
    diagnostics: http.post("diagnostics", {
      body: SessionIdParams,
      response: $type<GetSessionDiagnosticsResponse>(),
    }),
    worktree: http.resource("worktree", {
      /** Reads persisted worktree metadata attached to one daemon-managed session. */
      get: http.post("get", {
        body: GetSessionWorktreeRequest,
        response: $type<GetSessionWorktreeResponse>(),
      }),
    }),
    /** Shuts down one daemon-managed session and reports whether shutdown succeeded. */
    shutdown: http.post("shutdown", {
      body: SessionIdParams,
      response: $type<ShutdownSessionResponse>(),
    }),
    /** Cancels the active turn and returns any queued prompts the daemon aborted instead of replaying. */
    cancel: http.post("cancel", {
      body: CancelSessionRequest,
      response: $type<CancelSessionResponse>(),
    }),
    /** Cancels the active turn and injects one replacement prompt after the daemon observes a safe boundary. */
    steer: http.post("steer", {
      body: SteerSessionRequest,
      response: $type<SteerSessionResponse>(),
    }),
    /** Sends one raw message to a daemon-managed session and reports whether it was accepted. */
    send: http.post("send", {
      body: SendSessionMessageRequest,
      response: $type<{ accepted: true }>(),
    }),
    configOption: http.resource("config-option", {
      /** Updates one ACP config option on an active daemon-managed session. */
      set: http.post("set", {
        body: SetSessionConfigOptionRequest,
        response: $type<SetSessionConfigOptionResponse>(),
      }),
    }),
    model: http.resource("model", {
      /** Updates the ACP model on an active daemon-managed session. */
      set: http.post("set", {
        body: SetSessionModelRequest,
        response: $type<SetSessionModelResponse>(),
      }),
    }),
    /** Marks one session inbox row completed without shutting down the session. */
    complete: http.post("complete", {
      body: CompleteSessionRequest,
      response: $type<CompleteSessionResponse>(),
    }),
    /** Records the current session initiative without creating an inbox row. */
    declareInitiative: http.post("declare-initiative", {
      body: DeclareSessionInitiativeRequest,
      response: $type<ReportSessionResponse>(),
    }),
    /** Reports a session blocker and marks the session inbox row unread. */
    reportBlocker: http.post("report-blocker", {
      body: ReportSessionBlockerRequest,
      response: $type<ReportSessionResponse>(),
    }),
    /** Reports an end-of-turn session update when no other entity claimed attention. */
    reportTurnEnded: http.post("report-turn-ended", {
      body: ReportSessionTurnEndedRequest,
      response: $type<ReportSessionResponse>(),
    }),
    /** Resolves one daemon session token to its daemon session id. */
    resolveToken: http.post("resolve-token", {
      body: ResolveSessionTokenRequest,
      response: $type<{ id: string }>(),
    }),
    /** Streams live daemon-published ACP messages for one daemon-managed session id. */
    streamMessages: http.get("stream-messages", {
      query: SessionIdParams,
      response: ndjson.$type<SessionMessageEvent>(),
    }),
    /** Streams app-wide daemon session lifecycle updates without observing transcript messages. */
    streamLifecycle: http.get("stream-lifecycle", {
      response: ndjson.$type<SessionLifecycleEvent>(),
    }),
  }),
})
