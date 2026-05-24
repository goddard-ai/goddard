import { $type, defineIpcRoutes, http, ndjson } from "@goddard-ai/ipc"
import {
  CancelSessionRequest,
  CompleteSessionRequest,
  CreateSessionRequest,
  DeclareSessionInitiativeRequest,
  GetSessionChangesRequest as GetSessionChangesRequestSchema,
  GetSessionHistoryRequest as GetSessionHistoryRequestSchema,
  ListSessionsRequest,
  ReportSessionBlockerRequest,
  ReportSessionTurnEndedRequest,
  ResolveSessionTokenRequest,
  SendSessionMessageRequest,
  SessionComposerSuggestionsRequest,
  SessionDraftSuggestionsRequest,
  SessionLaunchPreviewRequest,
  SessionMessageEvent,
  SessionSubpackagesRequest,
  SteerSessionRequest,
  type CancelSessionResponse,
  type CompleteSessionResponse,
  type CreateSessionResponse,
  type GetSessionChangesResponse,
  type GetSessionDiagnosticsResponse,
  type GetSessionHistoryResponse,
  type GetSessionResponse,
  type GetSessionWorkforceResponse,
  type ListSessionsResponse,
  type ReportSessionResponse,
  type SessionComposerSuggestionsResponse,
  type SessionLaunchPreviewResponse,
  type SessionSubpackagesResponse,
  type ShutdownSessionResponse,
  type SteerSessionResponse,
} from "@goddard-ai/schema/daemon/sessions"
import { DaemonSessionIdParams } from "@goddard-ai/schema/id"

import {
  GetSessionWorktreeRequest,
  MountSessionReviewSessionRequest,
  RunSessionReviewSessionRequest,
  UnmountSessionReviewSessionRequest,
  type GetSessionWorktreeResponse,
  type MutateSessionReviewSessionResponse,
} from "./schema.ts"

export const sessionIpcRoutes = defineIpcRoutes({
  session: http.resource("session", {
    create: http.post("create", {
      body: CreateSessionRequest,
      response: $type<CreateSessionResponse>(),
    }),
    list: http.post("list", {
      body: ListSessionsRequest,
      response: $type<ListSessionsResponse>(),
    }),
    get: http.post("get", {
      body: DaemonSessionIdParams,
      response: $type<GetSessionResponse>(),
    }),
    connect: http.post("connect", {
      body: DaemonSessionIdParams,
      response: $type<GetSessionResponse>(),
    }),
    history: http.post("history", {
      body: GetSessionHistoryRequestSchema,
      response: $type<GetSessionHistoryResponse>(),
    }),
    changes: http.post("changes", {
      body: GetSessionChangesRequestSchema,
      response: $type<GetSessionChangesResponse>(),
    }),
    composerSuggestions: http.post("composer-suggestions", {
      body: SessionComposerSuggestionsRequest,
      response: $type<SessionComposerSuggestionsResponse>(),
    }),
    draftSuggestions: http.post("draft-suggestions", {
      body: SessionDraftSuggestionsRequest,
      response: $type<SessionComposerSuggestionsResponse>(),
    }),
    launchPreview: http.post("launch-preview", {
      body: SessionLaunchPreviewRequest,
      response: $type<SessionLaunchPreviewResponse>(),
    }),
    subpackages: http.post("subpackages", {
      body: SessionSubpackagesRequest,
      response: $type<SessionSubpackagesResponse>(),
    }),
    diagnostics: http.post("diagnostics", {
      body: DaemonSessionIdParams,
      response: $type<GetSessionDiagnosticsResponse>(),
    }),
    worktree: http.resource("worktree", {
      get: http.post("get", {
        body: GetSessionWorktreeRequest,
        response: $type<GetSessionWorktreeResponse>(),
      }),
    }),
    reviewSession: http.resource("review-session", {
      mount: http.post("mount", {
        body: MountSessionReviewSessionRequest,
        response: $type<MutateSessionReviewSessionResponse>(),
      }),
      run: http.post("run", {
        body: RunSessionReviewSessionRequest,
        response: $type<MutateSessionReviewSessionResponse>(),
      }),
      unmount: http.post("unmount", {
        body: UnmountSessionReviewSessionRequest,
        response: $type<MutateSessionReviewSessionResponse>(),
      }),
    }),
    workforce: http.resource("workforce", {
      get: http.post("get", {
        body: DaemonSessionIdParams,
        response: $type<GetSessionWorkforceResponse>(),
      }),
    }),
    shutdown: http.post("shutdown", {
      body: DaemonSessionIdParams,
      response: $type<ShutdownSessionResponse>(),
    }),
    cancel: http.post("cancel", {
      body: CancelSessionRequest,
      response: $type<CancelSessionResponse>(),
    }),
    steer: http.post("steer", {
      body: SteerSessionRequest,
      response: $type<SteerSessionResponse>(),
    }),
    send: http.post("send", {
      body: SendSessionMessageRequest,
      response: $type<{ accepted: true }>(),
    }),
    complete: http.post("complete", {
      body: CompleteSessionRequest,
      response: $type<CompleteSessionResponse>(),
    }),
    declareInitiative: http.post("declare-initiative", {
      body: DeclareSessionInitiativeRequest,
      response: $type<ReportSessionResponse>(),
    }),
    reportBlocker: http.post("report-blocker", {
      body: ReportSessionBlockerRequest,
      response: $type<ReportSessionResponse>(),
    }),
    reportTurnEnded: http.post("report-turn-ended", {
      body: ReportSessionTurnEndedRequest,
      response: $type<ReportSessionResponse>(),
    }),
    resolveToken: http.post("resolve-token", {
      body: ResolveSessionTokenRequest,
      response: $type<{ id: string }>(),
    }),
    messageEvents: http.get("message-events", {
      query: DaemonSessionIdParams,
      response: ndjson.$type<SessionMessageEvent>(),
    }),
  }),
})
