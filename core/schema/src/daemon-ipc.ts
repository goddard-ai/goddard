import { $type } from "@goddard-ai/ipc"
import { z } from "zod"
import { DaemonSessionIdParams } from "./common/params.js"
import { CreateDaemonSessionRequest } from "./daemon/sessions.js"
import { ReplyPrDaemonRequest, SubmitPrDaemonRequest } from "./daemon/pull-requests.js"
import type {
  CreateDaemonSessionResponse,
  DaemonHealth,
  GetDaemonSessionDiagnosticsResponse,
  GetDaemonSessionHistoryResponse,
  GetDaemonSessionResponse,
  GetDaemonWorkforceResponse,
  ListDaemonWorkforcesResponse,
  MutateDaemonWorkforceResponse,
  ReplyPrDaemonResponse,
  ShutdownDaemonSessionResponse,
  ShutdownDaemonWorkforceResponse,
  StartDaemonWorkforceResponse,
  SubmitPrDaemonResponse,
} from "./daemon.js"
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
} from "./workforce/requests.js"

/** IPC contract map shared by the daemon client and server. */
export const daemonIpcSchema = {
  client: {
    requests: {
      health: {
        payload: z.object({}),
        response: $type<DaemonHealth>(),
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
