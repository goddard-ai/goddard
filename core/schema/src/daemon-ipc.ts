import { $type, IpcSchema } from "@goddard-ai/ipc"
import { z } from "zod"
import { AuthSession, DeviceFlowComplete, DeviceFlowSession, DeviceFlowStart } from "./backend.ts"
import { DaemonSessionIdParams } from "./common/params.ts"
import { type ListAdaptersResponse, ListAdaptersRequest } from "./daemon-adapters.ts"
import { RunNamedActionRequest } from "./daemon/actions.ts"
import {
  type GetLoopResponse,
  type ListLoopsResponse,
  type ShutdownLoopResponse,
  type StartLoopResponse,
  GetLoopRequest,
  ShutdownLoopRequest,
  StartLoopRequest,
} from "./daemon/loops.ts"
import {
  type ReplyPrResponse,
  type SubmitPrResponse,
  ReplyPrRequest,
  SubmitPrRequest,
} from "./daemon/pull-requests.ts"
import {
  type CancelSessionResponse,
  type CreateSessionResponse,
  type GetSessionDiagnosticsResponse,
  type GetSessionHistoryResponse,
  type GetSessionResponse,
  type GetSessionWorkforceResponse,
  type GetSessionWorktreeResponse,
  type ListSessionsResponse,
  type MountSessionWorktreeSyncRequest,
  type MutateSessionWorktreeResponse,
  type ShutdownSessionResponse,
  type SyncSessionWorktreeRequest,
  type SteerSessionResponse,
  type UnmountSessionWorktreeSyncRequest,
  CancelSessionRequest,
  CreateSessionRequest,
  ListSessionsRequest,
  MountSessionWorktreeSyncRequest as MountSessionWorktreeSyncRequestSchema,
  ResolveSessionTokenRequest,
  SendSessionMessageRequest,
  SessionMessageEvent,
  SyncSessionWorktreeRequest as SyncSessionWorktreeRequestSchema,
  SteerSessionRequest,
  UnmountSessionWorktreeSyncRequest as UnmountSessionWorktreeSyncRequestSchema,
} from "./daemon/sessions.ts"
import {
  type DiscoverWorkforceCandidatesResponse,
  type GetWorkforceResponse,
  type InitializeWorkforceResponse,
  type ListWorkforcesResponse,
  type MutateWorkforceResponse,
  type ShutdownWorkforceResponse,
  type StartWorkforceResponse,
  type WorkforceEventEnvelope,
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
      payload: DaemonSessionIdParams,
      response: $type<GetSessionHistoryResponse>(),
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
    sessionResolveToken: {
      payload: ResolveSessionTokenRequest,
      response: $type<{ id: string }>(),
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
