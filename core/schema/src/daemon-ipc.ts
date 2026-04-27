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
    health: {
      response: $type<{ ok: boolean }>(),
    },
    authDeviceStart: {
      payload: DeviceFlowStart,
      response: $type<DeviceFlowSession>(),
    },
    authDeviceComplete: {
      payload: DeviceFlowComplete,
      response: $type<AuthSession>(),
    },
    authWhoami: {
      response: $type<AuthSession>(),
    },
    authLogout: {
      response: $type<{ success: true }>(),
    },
    adapterList: {
      payload: ListAdaptersRequest,
      response: $type<ListAdaptersResponse>(),
    },
    prSubmit: {
      payload: SubmitPrRequest.extend({
        token: z.string(),
      }),
      response: $type<SubmitPrResponse>(),
    },
    prGet: {
      payload: GetPullRequestRequest,
      response: $type<GetPullRequestResponse>(),
    },
    prReply: {
      payload: ReplyPrRequest.extend({
        token: z.string(),
      }),
      response: $type<ReplyPrResponse>(),
    },
    sessionCreate: {
      payload: CreateSessionRequest,
      response: $type<CreateSessionResponse>(),
    },
    sessionList: {
      payload: ListSessionsRequest,
      response: $type<ListSessionsResponse>(),
    },
    sessionGet: {
      payload: DaemonSessionIdParams,
      response: $type<GetSessionResponse>(),
    },
    sessionConnect: {
      payload: DaemonSessionIdParams,
      response: $type<GetSessionResponse>(),
    },
    sessionHistory: {
      payload: GetSessionHistoryRequestSchema,
      response: $type<GetSessionHistoryResponse>(),
    },
    sessionChanges: {
      payload: GetSessionChangesRequestSchema,
      response: $type<GetSessionChangesResponse>(),
    },
    sessionComposerSuggestions: {
      payload: SessionComposerSuggestionsRequest,
      response: $type<SessionComposerSuggestionsResponse>(),
    },
    sessionDraftSuggestions: {
      payload: SessionDraftSuggestionsRequest,
      response: $type<SessionComposerSuggestionsResponse>(),
    },
    sessionLaunchPreview: {
      payload: SessionLaunchPreviewRequest,
      response: $type<SessionLaunchPreviewResponse>(),
    },
    sessionDiagnostics: {
      payload: DaemonSessionIdParams,
      response: $type<GetSessionDiagnosticsResponse>(),
    },
    sessionWorktree: {
      payload: DaemonSessionIdParams,
      response: $type<GetSessionWorktreeResponse>(),
    },
    sessionWorktreeSyncMount: {
      payload: MountSessionWorktreeSyncRequestSchema,
      response: $type<MutateSessionWorktreeResponse>(),
    },
    sessionWorktreeSync: {
      payload: SyncSessionWorktreeRequestSchema,
      response: $type<MutateSessionWorktreeResponse>(),
    },
    sessionWorktreeSyncUnmount: {
      payload: UnmountSessionWorktreeSyncRequestSchema,
      response: $type<MutateSessionWorktreeResponse>(),
    },
    sessionWorkforce: {
      payload: DaemonSessionIdParams,
      response: $type<GetSessionWorkforceResponse>(),
    },
    sessionShutdown: {
      payload: DaemonSessionIdParams,
      response: $type<ShutdownSessionResponse>(),
    },
    sessionCancel: {
      payload: CancelSessionRequest,
      response: $type<CancelSessionResponse>(),
    },
    sessionSteer: {
      payload: SteerSessionRequest,
      response: $type<SteerSessionResponse>(),
    },
    sessionSend: {
      payload: SendSessionMessageRequest,
      response: $type<{ accepted: true }>(),
    },
    sessionComplete: {
      payload: CompleteSessionRequest,
      response: $type<CompleteSessionResponse>(),
    },
    sessionDeclareInitiative: {
      payload: DeclareSessionInitiativeRequest,
      response: $type<ReportSessionResponse>(),
    },
    sessionReportBlocker: {
      payload: ReportSessionBlockerRequest,
      response: $type<ReportSessionResponse>(),
    },
    sessionReportTurnEnded: {
      payload: ReportSessionTurnEndedRequest,
      response: $type<ReportSessionResponse>(),
    },
    sessionResolveToken: {
      payload: ResolveSessionTokenRequest,
      response: $type<{ id: string }>(),
    },
    inboxList: {
      payload: ListInboxRequest,
      response: $type<ListInboxResponse>(),
    },
    inboxUpdate: {
      payload: UpdateInboxItemRequest,
      response: $type<UpdateInboxItemResponse>(),
    },
    inboxBulkUpdate: {
      payload: BulkUpdateInboxItemsRequest,
      response: $type<BulkUpdateInboxItemsResponse>(),
    },
    actionRun: {
      payload: RunNamedActionRequest,
      response: $type<CreateSessionResponse>(),
    },
    loopStart: {
      payload: StartLoopRequest,
      response: $type<StartLoopResponse>(),
    },
    loopGet: {
      payload: GetLoopRequest,
      response: $type<GetLoopResponse>(),
    },
    loopList: {
      response: $type<ListLoopsResponse>(),
    },
    loopShutdown: {
      payload: ShutdownLoopRequest,
      response: $type<ShutdownLoopResponse>(),
    },
    workforceStart: {
      payload: StartWorkforceRequest,
      response: $type<StartWorkforceResponse>(),
    },
    workforceDiscoverCandidates: {
      payload: DiscoverWorkforceCandidatesRequest,
      response: $type<DiscoverWorkforceCandidatesResponse>(),
    },
    workforceInitialize: {
      payload: InitializeWorkforceRequest,
      response: $type<InitializeWorkforceResponse>(),
    },
    workforceGet: {
      payload: GetWorkforceRequest,
      response: $type<GetWorkforceResponse>(),
    },
    workforceList: {
      response: $type<ListWorkforcesResponse>(),
    },
    workforceShutdown: {
      payload: ShutdownWorkforceRequest,
      response: $type<ShutdownWorkforceResponse>(),
    },
    workforceRequest: {
      payload: CreateWorkforceRequest,
      response: $type<MutateWorkforceResponse>(),
    },
    workforceUpdate: {
      payload: UpdateWorkforceRequest,
      response: $type<MutateWorkforceResponse>(),
    },
    workforceCancel: {
      payload: CancelWorkforceRequest,
      response: $type<MutateWorkforceResponse>(),
    },
    workforceTruncate: {
      payload: TruncateWorkforceRequest,
      response: $type<MutateWorkforceResponse>(),
    },
    workforceRespond: {
      payload: RespondWorkforceRequest,
      response: $type<MutateWorkforceResponse>(),
    },
    workforceSuspend: {
      payload: SuspendWorkforceRequest,
      response: $type<MutateWorkforceResponse>(),
    },
  },
  streams: {
    sessionMessage: {
      payload: $type<SessionMessageEvent>(),
      filter: DaemonSessionIdParams,
    },
    workforceEvent: {
      payload: $type<WorkforceEventEnvelope>(),
      filter: SubscribeWorkforceEventsRequest,
    },
  },
} satisfies IpcSchema
