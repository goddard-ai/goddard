import { $type } from "@goddard-ai/ipc"
import { AuthSession, DeviceFlowComplete, DeviceFlowSession, DeviceFlowStart } from "./backend.ts"
import { z } from "zod"
import { DaemonSessionIdParams } from "./common/params.ts"
import type {
  CreateDaemonSessionResponse,
  DaemonHealth,
  GetDaemonLoopResponse,
  GetDaemonSessionDiagnosticsResponse,
  GetDaemonSessionHistoryResponse,
  GetDaemonSessionResponse,
  GetDaemonWorkforceResponse,
  ListDaemonLoopsResponse,
  ListDaemonSessionsResponse,
  ListDaemonWorkforcesResponse,
  MutateDaemonWorkforceResponse,
  ReplyPrDaemonResponse,
  ShutdownDaemonLoopResponse,
  ShutdownDaemonSessionResponse,
  ShutdownDaemonWorkforceResponse,
  StartDaemonLoopResponse,
  StartDaemonWorkforceResponse,
  SubmitPrDaemonResponse,
} from "./daemon.ts"
import { RunNamedDaemonActionRequest } from "./daemon/actions.ts"
import {
  GetDaemonLoopRequest,
  ShutdownDaemonLoopRequest,
  StartDaemonLoopRequest,
} from "./daemon/loops.ts"
import { ReplyPrDaemonRequest, SubmitPrDaemonRequest } from "./daemon/pull-requests.ts"
import { CreateDaemonSessionRequest, ListDaemonSessionsRequest } from "./daemon/sessions.ts"
import {
  CancelDaemonWorkforceRequest,
  CreateDaemonWorkforceRequestRequest,
  GetDaemonWorkforceRequest,
  RespondDaemonWorkforceRequest,
  ShutdownDaemonWorkforceRequest,
  StartDaemonWorkforceRequest,
  SuspendDaemonWorkforceRequest,
  TruncateDaemonWorkforceRequest,
  UpdateDaemonWorkforceRequest,
} from "./workforce/requests.ts"

/** IPC contract map shared by the daemon client and server. */
export const daemonIpcSchema = {
  client: {
    requests: {
      health: {
        payload: z.object({}),
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
        payload: z.object({}),
        response: $type<AuthSession>(),
      },
      authLogout: {
        payload: z.object({}),
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
      sessionShutdown: {
        payload: DaemonSessionIdParams,
        response: $type<ShutdownDaemonSessionResponse>(),
      },
      sessionSend: {
        payload: z.object({
          id: z.string(),
          message: z.unknown(),
        }),
        response: $type<{ accepted: true }>(),
      },
      sessionResolveToken: {
        payload: z.object({ token: z.string() }),
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
        payload: z.object({}),
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
      workforceGet: {
        payload: GetDaemonWorkforceRequest,
        response: $type<GetDaemonWorkforceResponse>(),
      },
      workforceList: {
        payload: z.object({}),
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
  },
  server: {
    streams: {
      sessionMessage: z.object({
        id: z.string(),
        message: z.unknown(),
      }),
    },
  },
}
