import { $type, defineIpcSchema } from "@goddard-ai/ipc"
import { DaemonSessionIdParams } from "@goddard-ai/schema/common/params"
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

import {
  GetSessionWorktreeRequest,
  MountSessionReviewSessionRequest,
  RunSessionReviewSessionRequest,
  UnmountSessionReviewSessionRequest,
  type GetSessionWorktreeResponse,
  type MutateSessionReviewSessionResponse,
} from "./schema.ts"

export const sessionIpcSchema = defineIpcSchema({
  requests: {
    "session.create": {
      payload: CreateSessionRequest,
      response: $type<CreateSessionResponse>(),
    },
    "session.list": {
      payload: ListSessionsRequest,
      response: $type<ListSessionsResponse>(),
    },
    "session.get": {
      payload: DaemonSessionIdParams,
      response: $type<GetSessionResponse>(),
    },
    "session.connect": {
      payload: DaemonSessionIdParams,
      response: $type<GetSessionResponse>(),
    },
    "session.history": {
      payload: GetSessionHistoryRequestSchema,
      response: $type<GetSessionHistoryResponse>(),
    },
    "session.changes": {
      payload: GetSessionChangesRequestSchema,
      response: $type<GetSessionChangesResponse>(),
    },
    "session.composerSuggestions": {
      payload: SessionComposerSuggestionsRequest,
      response: $type<SessionComposerSuggestionsResponse>(),
    },
    "session.draftSuggestions": {
      payload: SessionDraftSuggestionsRequest,
      response: $type<SessionComposerSuggestionsResponse>(),
    },
    "session.launchPreview": {
      payload: SessionLaunchPreviewRequest,
      response: $type<SessionLaunchPreviewResponse>(),
    },
    "session.subpackages": {
      payload: SessionSubpackagesRequest,
      response: $type<SessionSubpackagesResponse>(),
    },
    "session.diagnostics": {
      payload: DaemonSessionIdParams,
      response: $type<GetSessionDiagnosticsResponse>(),
    },
    "session.worktree.get": {
      payload: GetSessionWorktreeRequest,
      response: $type<GetSessionWorktreeResponse>(),
    },
    "session.reviewSession.mount": {
      payload: MountSessionReviewSessionRequest,
      response: $type<MutateSessionReviewSessionResponse>(),
    },
    "session.reviewSession.run": {
      payload: RunSessionReviewSessionRequest,
      response: $type<MutateSessionReviewSessionResponse>(),
    },
    "session.reviewSession.unmount": {
      payload: UnmountSessionReviewSessionRequest,
      response: $type<MutateSessionReviewSessionResponse>(),
    },
    "session.workforce.get": {
      payload: DaemonSessionIdParams,
      response: $type<GetSessionWorkforceResponse>(),
    },
    "session.shutdown": {
      payload: DaemonSessionIdParams,
      response: $type<ShutdownSessionResponse>(),
    },
    "session.cancel": {
      payload: CancelSessionRequest,
      response: $type<CancelSessionResponse>(),
    },
    "session.steer": {
      payload: SteerSessionRequest,
      response: $type<SteerSessionResponse>(),
    },
    "session.send": {
      payload: SendSessionMessageRequest,
      response: $type<{ accepted: true }>(),
    },
    "session.complete": {
      payload: CompleteSessionRequest,
      response: $type<CompleteSessionResponse>(),
    },
    "session.declareInitiative": {
      payload: DeclareSessionInitiativeRequest,
      response: $type<ReportSessionResponse>(),
    },
    "session.reportBlocker": {
      payload: ReportSessionBlockerRequest,
      response: $type<ReportSessionResponse>(),
    },
    "session.reportTurnEnded": {
      payload: ReportSessionTurnEndedRequest,
      response: $type<ReportSessionResponse>(),
    },
    "session.resolveToken": {
      payload: ResolveSessionTokenRequest,
      response: $type<{ id: string }>(),
    },
  },
  streams: {
    "session.message": {
      payload: $type<SessionMessageEvent>(),
      filter: DaemonSessionIdParams,
    },
  },
})
