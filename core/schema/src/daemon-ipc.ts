import { $type, IpcSchema } from "@goddard-ai/ipc"
import { z } from "zod"
import { AuthSession, DeviceFlowComplete, DeviceFlowSession, DeviceFlowStart } from "./backend.ts"
import { DaemonSessionIdParams } from "./common/params.ts"
import { RunNamedDaemonActionRequest } from "./daemon/actions.ts"
import type { DaemonHealth } from "./daemon/health.ts"
import {
  type GetDaemonLoopResponse,
  type ListDaemonLoopsResponse,
  type ShutdownDaemonLoopResponse,
  type StartDaemonLoopResponse,
  GetDaemonLoopRequest,
  ShutdownDaemonLoopRequest,
  StartDaemonLoopRequest,
} from "./daemon/loops.ts"
import {
  type ReplyPrDaemonResponse,
  type SubmitPrDaemonResponse,
  ReplyPrDaemonRequest,
  SubmitPrDaemonRequest,
} from "./daemon/pull-requests.ts"
import {
  type CancelDaemonSessionResponse,
  type CreateDaemonSessionResponse,
  type GetDaemonSessionDiagnosticsResponse,
  type GetDaemonSessionHistoryResponse,
  type GetDaemonSessionResponse,
  type GetDaemonSessionWorkforceResponse,
  type GetDaemonSessionWorktreeResponse,
  type ListDaemonSessionsResponse,
  type ShutdownDaemonSessionResponse,
  type SteerDaemonSessionResponse,
  CancelDaemonSessionRequest,
  CreateDaemonSessionRequest,
  DaemonSessionMessageEvent,
  ListDaemonSessionsRequest,
  ResolveDaemonSessionTokenRequest,
  SendDaemonSessionMessageRequest,
  SteerDaemonSessionRequest,
} from "./daemon/sessions.ts"
import {
  type DiscoverDaemonWorkforceCandidatesResponse,
  type DaemonWorkforceEvent,
  type GetDaemonWorkforceResponse,
  type InitializeDaemonWorkforceResponse,
  type ListDaemonWorkforcesResponse,
  type MutateDaemonWorkforceResponse,
  type ShutdownDaemonWorkforceResponse,
  type StartDaemonWorkforceResponse,
  CancelDaemonWorkforceRequest,
  CreateDaemonWorkforceRequestRequest,
  DiscoverDaemonWorkforceCandidatesRequest,
  GetDaemonWorkforceRequest,
  InitializeDaemonWorkforceRequest,
  RespondDaemonWorkforceRequest,
  ShutdownDaemonWorkforceRequest,
  StartDaemonWorkforceRequest,
  SubscribeDaemonWorkforceEventsRequest,
  SuspendDaemonWorkforceRequest,
  TruncateDaemonWorkforceRequest,
  UpdateDaemonWorkforceRequest,
} from "./workforce/requests.ts"

/** IPC contract map shared by the daemon client and server. */
export const daemonIpcSchema = {
  requests: {
    health: {
      response: $type<DaemonHealth>(),
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
    prSubmit: {
      payload: SubmitPrDaemonRequest.extend({
        token: z.string(),
      }),
      response: $type<SubmitPrDaemonResponse>(),
    },
    prReply: {
      payload: ReplyPrDaemonRequest.extend({
        token: z.string(),
      }),
      response: $type<ReplyPrDaemonResponse>(),
    },
    sessionCreate: {
      payload: CreateDaemonSessionRequest,
      response: $type<CreateDaemonSessionResponse>(),
    },
    sessionList: {
      payload: ListDaemonSessionsRequest,
      response: $type<ListDaemonSessionsResponse>(),
    },
    sessionGet: {
      payload: DaemonSessionIdParams,
      response: $type<GetDaemonSessionResponse>(),
    },
    sessionConnect: {
      payload: DaemonSessionIdParams,
      response: $type<GetDaemonSessionResponse>(),
    },
    sessionHistory: {
      payload: DaemonSessionIdParams,
      response: $type<GetDaemonSessionHistoryResponse>(),
    },
    sessionDiagnostics: {
      payload: DaemonSessionIdParams,
      response: $type<GetDaemonSessionDiagnosticsResponse>(),
    },
    sessionWorktree: {
      payload: DaemonSessionIdParams,
      response: $type<GetDaemonSessionWorktreeResponse>(),
    },
    sessionWorkforce: {
      payload: DaemonSessionIdParams,
      response: $type<GetDaemonSessionWorkforceResponse>(),
    },
    sessionShutdown: {
      payload: DaemonSessionIdParams,
      response: $type<ShutdownDaemonSessionResponse>(),
    },
    sessionCancel: {
      payload: CancelDaemonSessionRequest,
      response: $type<CancelDaemonSessionResponse>(),
    },
    sessionSteer: {
      payload: SteerDaemonSessionRequest,
      response: $type<SteerDaemonSessionResponse>(),
    },
    sessionSend: {
      payload: SendDaemonSessionMessageRequest,
      response: $type<{ accepted: true }>(),
    },
    sessionResolveToken: {
      payload: ResolveDaemonSessionTokenRequest,
      response: $type<{ id: string }>(),
    },
    actionRun: {
      payload: RunNamedDaemonActionRequest,
      response: $type<CreateDaemonSessionResponse>(),
    },
    loopStart: {
      payload: StartDaemonLoopRequest,
      response: $type<StartDaemonLoopResponse>(),
    },
    loopGet: {
      payload: GetDaemonLoopRequest,
      response: $type<GetDaemonLoopResponse>(),
    },
    loopList: {
      response: $type<ListDaemonLoopsResponse>(),
    },
    loopShutdown: {
      payload: ShutdownDaemonLoopRequest,
      response: $type<ShutdownDaemonLoopResponse>(),
    },
    workforceStart: {
      payload: StartDaemonWorkforceRequest,
      response: $type<StartDaemonWorkforceResponse>(),
    },
    workforceDiscoverCandidates: {
      payload: DiscoverDaemonWorkforceCandidatesRequest,
      response: $type<DiscoverDaemonWorkforceCandidatesResponse>(),
    },
    workforceInitialize: {
      payload: InitializeDaemonWorkforceRequest,
      response: $type<InitializeDaemonWorkforceResponse>(),
    },
    workforceGet: {
      payload: GetDaemonWorkforceRequest,
      response: $type<GetDaemonWorkforceResponse>(),
    },
    workforceList: {
      response: $type<ListDaemonWorkforcesResponse>(),
    },
    workforceShutdown: {
      payload: ShutdownDaemonWorkforceRequest,
      response: $type<ShutdownDaemonWorkforceResponse>(),
    },
    workforceRequest: {
      payload: CreateDaemonWorkforceRequestRequest,
      response: $type<MutateDaemonWorkforceResponse>(),
    },
    workforceUpdate: {
      payload: UpdateDaemonWorkforceRequest,
      response: $type<MutateDaemonWorkforceResponse>(),
    },
    workforceCancel: {
      payload: CancelDaemonWorkforceRequest,
      response: $type<MutateDaemonWorkforceResponse>(),
    },
    workforceTruncate: {
      payload: TruncateDaemonWorkforceRequest,
      response: $type<MutateDaemonWorkforceResponse>(),
    },
    workforceRespond: {
      payload: RespondDaemonWorkforceRequest,
      response: $type<MutateDaemonWorkforceResponse>(),
    },
    workforceSuspend: {
      payload: SuspendDaemonWorkforceRequest,
      response: $type<MutateDaemonWorkforceResponse>(),
    },
  },
  streams: {
    sessionMessage: {
      payload: $type<DaemonSessionMessageEvent>(),
      filter: DaemonSessionIdParams,
    },
    workforceEvent: {
      payload: $type<DaemonWorkforceEvent>(),
      filter: SubscribeDaemonWorkforceEventsRequest,
    },
  },
} satisfies IpcSchema
