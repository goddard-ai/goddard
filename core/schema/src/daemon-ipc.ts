import { $type } from "@goddard-ai/ipc"
import { AuthSession, DeviceFlowComplete, DeviceFlowSession, DeviceFlowStart } from "./backend.ts"
import { z } from "zod"
import { DaemonSessionIdParams } from "./common/params.ts"
import type { DaemonHealth } from "./daemon/health.ts"
import { GetDaemonHealthRequest } from "./daemon/health.ts"
import { RunNamedDaemonActionRequest } from "./daemon/actions.ts"
import {
  type GetDaemonLoopResponse,
  GetDaemonLoopRequest,
  type ListDaemonLoopsResponse,
  type ShutdownDaemonLoopResponse,
  ShutdownDaemonLoopRequest,
  type StartDaemonLoopResponse,
  StartDaemonLoopRequest,
} from "./daemon/loops.ts"
import {
  type ReplyPrDaemonResponse,
  ReplyPrDaemonRequest,
  type SubmitPrDaemonResponse,
  SubmitPrDaemonRequest,
} from "./daemon/pull-requests.ts"
import {
  type CreateDaemonSessionResponse,
  CreateDaemonSessionRequest,
  type GetDaemonSessionDiagnosticsResponse,
  type GetDaemonSessionHistoryResponse,
  type GetDaemonSessionResponse,
  type ListDaemonSessionsResponse,
  ListDaemonSessionsRequest,
  type ShutdownDaemonSessionResponse,
} from "./daemon/sessions.ts"
import {
  CancelDaemonWorkforceRequest,
  CreateDaemonWorkforceRequestRequest,
  type GetDaemonWorkforceResponse,
  GetDaemonWorkforceRequest,
  type ListDaemonWorkforcesResponse,
  type MutateDaemonWorkforceResponse,
  RespondDaemonWorkforceRequest,
  type ShutdownDaemonWorkforceResponse,
  ShutdownDaemonWorkforceRequest,
  type StartDaemonWorkforceResponse,
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
        payload: GetDaemonHealthRequest,
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
