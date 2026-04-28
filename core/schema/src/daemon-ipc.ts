import { $type, IpcSchema } from "@goddard-ai/ipc"
import { z } from "zod"

import { AuthSession, DeviceFlowComplete, DeviceFlowSession, DeviceFlowStart } from "./backend.ts"
import { DaemonSessionIdParams } from "./common/params.ts"
import { ListAdaptersRequest, type ListAdaptersResponse } from "./daemon-adapters.ts"
import { RunNamedActionRequest } from "./daemon/actions.ts"
import {
  BulkUpdateInboxItemsRequest,
  ListInboxRequest,
  UpdateInboxItemRequest,
  type BulkUpdateInboxItemsResponse,
  type ListInboxResponse,
  type UpdateInboxItemResponse,
} from "./daemon/inbox.ts"
import {
  GetLoopRequest,
  ShutdownLoopRequest,
  StartLoopRequest,
  type GetLoopResponse,
  type ListLoopsResponse,
  type ShutdownLoopResponse,
  type StartLoopResponse,
} from "./daemon/loops.ts"
import {
  GetPullRequestRequest,
  ReplyPrRequest,
  SubmitPrRequest,
  type GetPullRequestResponse,
  type ReplyPrResponse,
  type SubmitPrResponse,
} from "./daemon/pull-requests.ts"
import {
  CancelSessionRequest,
  CompleteSessionRequest,
  CreateSessionRequest,
  DeclareSessionInitiativeRequest,
  GetSessionChangesRequest as GetSessionChangesRequestSchema,
  GetSessionHistoryRequest as GetSessionHistoryRequestSchema,
  ListSessionsRequest,
  MountSessionWorktreeSyncRequest as MountSessionWorktreeSyncRequestSchema,
  ReportSessionBlockerRequest,
  ReportSessionTurnEndedRequest,
  ResolveSessionTokenRequest,
  SendSessionMessageRequest,
  SessionComposerSuggestionsRequest,
  SessionDraftSuggestionsRequest,
  SessionLaunchPreviewRequest,
  SessionMessageEvent,
  SteerSessionRequest,
  SyncSessionWorktreeRequest as SyncSessionWorktreeRequestSchema,
  UnmountSessionWorktreeSyncRequest as UnmountSessionWorktreeSyncRequestSchema,
  type CancelSessionResponse,
  type CompleteSessionResponse,
  type CreateSessionResponse,
  type GetSessionChangesResponse,
  type GetSessionDiagnosticsResponse,
  type GetSessionHistoryResponse,
  type GetSessionResponse,
  type GetSessionWorkforceResponse,
  type GetSessionWorktreeResponse,
  type ListSessionsResponse,
  type MutateSessionWorktreeResponse,
  type ReportSessionResponse,
  type SessionComposerSuggestionsResponse,
  type SessionLaunchPreviewResponse,
  type ShutdownSessionResponse,
  type SteerSessionResponse,
} from "./daemon/sessions.ts"
import {
  CancelWorkforceRequest,
  CreateWorkforceRequest,
  DiscoverWorkforceCandidatesRequest,
  GetWorkforceRequest,
  InitializeWorkforceRequest,
  RespondWorkforceRequest,
  ShutdownWorkforceRequest,
  StartWorkforceRequest,
  SubscribeWorkforceEventsRequest,
  SuspendWorkforceRequest,
  TruncateWorkforceRequest,
  UpdateWorkforceRequest,
  type DiscoverWorkforceCandidatesResponse,
  type GetWorkforceResponse,
  type InitializeWorkforceResponse,
  type ListWorkforcesResponse,
  type MutateWorkforceResponse,
  type ShutdownWorkforceResponse,
  type StartWorkforceResponse,
  type WorkforceEventEnvelope,
} from "./workforce/requests.ts"

/** IPC contract map shared by the daemon client and server. */
export const daemonIpcSchema = {
  requests: {
    "daemon.health": {
      response: $type<{ ok: boolean }>(),
    },
    "auth.device.start": {
      payload: DeviceFlowStart,
      response: $type<DeviceFlowSession>(),
    },
    "auth.device.complete": {
      payload: DeviceFlowComplete,
      response: $type<AuthSession>(),
    },
    "auth.whoami": {
      response: $type<AuthSession>(),
    },
    "auth.logout": {
      response: $type<{ success: true }>(),
    },
    "adapter.list": {
      payload: ListAdaptersRequest,
      response: $type<ListAdaptersResponse>(),
    },
    "pr.submit": {
      payload: SubmitPrRequest.extend({
        token: z.string(),
      }),
      response: $type<SubmitPrResponse>(),
    },
    "pr.get": {
      payload: GetPullRequestRequest,
      response: $type<GetPullRequestResponse>(),
    },
    "pr.reply": {
      payload: ReplyPrRequest.extend({
        token: z.string(),
      }),
      response: $type<ReplyPrResponse>(),
    },
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
    "session.diagnostics": {
      payload: DaemonSessionIdParams,
      response: $type<GetSessionDiagnosticsResponse>(),
    },
    "session.worktree.get": {
      payload: DaemonSessionIdParams,
      response: $type<GetSessionWorktreeResponse>(),
    },
    "session.worktreeSync.mount": {
      payload: MountSessionWorktreeSyncRequestSchema,
      response: $type<MutateSessionWorktreeResponse>(),
    },
    "session.worktreeSync.run": {
      payload: SyncSessionWorktreeRequestSchema,
      response: $type<MutateSessionWorktreeResponse>(),
    },
    "session.worktreeSync.unmount": {
      payload: UnmountSessionWorktreeSyncRequestSchema,
      response: $type<MutateSessionWorktreeResponse>(),
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
    "inbox.list": {
      payload: ListInboxRequest,
      response: $type<ListInboxResponse>(),
    },
    "inbox.update": {
      payload: UpdateInboxItemRequest,
      response: $type<UpdateInboxItemResponse>(),
    },
    "inbox.bulkUpdate": {
      payload: BulkUpdateInboxItemsRequest,
      response: $type<BulkUpdateInboxItemsResponse>(),
    },
    "action.run": {
      payload: RunNamedActionRequest,
      response: $type<CreateSessionResponse>(),
    },
    "loop.start": {
      payload: StartLoopRequest,
      response: $type<StartLoopResponse>(),
    },
    "loop.get": {
      payload: GetLoopRequest,
      response: $type<GetLoopResponse>(),
    },
    "loop.list": {
      response: $type<ListLoopsResponse>(),
    },
    "loop.shutdown": {
      payload: ShutdownLoopRequest,
      response: $type<ShutdownLoopResponse>(),
    },
    "workforce.start": {
      payload: StartWorkforceRequest,
      response: $type<StartWorkforceResponse>(),
    },
    "workforce.discoverCandidates": {
      payload: DiscoverWorkforceCandidatesRequest,
      response: $type<DiscoverWorkforceCandidatesResponse>(),
    },
    "workforce.initialize": {
      payload: InitializeWorkforceRequest,
      response: $type<InitializeWorkforceResponse>(),
    },
    "workforce.get": {
      payload: GetWorkforceRequest,
      response: $type<GetWorkforceResponse>(),
    },
    "workforce.list": {
      response: $type<ListWorkforcesResponse>(),
    },
    "workforce.shutdown": {
      payload: ShutdownWorkforceRequest,
      response: $type<ShutdownWorkforceResponse>(),
    },
    "workforce.request": {
      payload: CreateWorkforceRequest,
      response: $type<MutateWorkforceResponse>(),
    },
    "workforce.update": {
      payload: UpdateWorkforceRequest,
      response: $type<MutateWorkforceResponse>(),
    },
    "workforce.cancel": {
      payload: CancelWorkforceRequest,
      response: $type<MutateWorkforceResponse>(),
    },
    "workforce.truncate": {
      payload: TruncateWorkforceRequest,
      response: $type<MutateWorkforceResponse>(),
    },
    "workforce.respond": {
      payload: RespondWorkforceRequest,
      response: $type<MutateWorkforceResponse>(),
    },
    "workforce.suspend": {
      payload: SuspendWorkforceRequest,
      response: $type<MutateWorkforceResponse>(),
    },
  },
  streams: {
    "session.message": {
      payload: $type<SessionMessageEvent>(),
      filter: DaemonSessionIdParams,
    },
    "workforce.event": {
      payload: $type<WorkforceEventEnvelope>(),
      filter: SubscribeWorkforceEventsRequest,
    },
  },
} satisfies IpcSchema
