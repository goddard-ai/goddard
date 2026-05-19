import { adapterIpcSchema } from "@goddard-ai/adapter/daemon-ipc"
import { authIpcSchema } from "@goddard-ai/auth/daemon-ipc"
import { inboxIpcSchema } from "@goddard-ai/inbox/daemon-ipc"
import { $type, composeIpcSchemas, defineIpcSchema, IpcSchema } from "@goddard-ai/ipc"
import { sessionIpcSchema } from "@goddard-ai/session/daemon-ipc"
import { z } from "zod"

import { RunNamedActionRequest } from "./daemon/actions.ts"
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
import { type CreateSessionResponse } from "./daemon/sessions.ts"
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

const coreDaemonIpcSchema = defineIpcSchema({
  requests: {
    "daemon.health": {
      response: $type<{ ok: boolean }>(),
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
    "workforce.event": {
      payload: $type<WorkforceEventEnvelope>(),
      filter: SubscribeWorkforceEventsRequest,
    },
  },
})

/** IPC contract map shared by the daemon client and server. */
export const daemonIpcSchema = composeIpcSchemas([
  coreDaemonIpcSchema,
  adapterIpcSchema,
  authIpcSchema,
  inboxIpcSchema,
  sessionIpcSchema,
]) satisfies IpcSchema
