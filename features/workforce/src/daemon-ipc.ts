import { $type, defineIpcRoutes, http, ndjson } from "@goddard-ai/ipc"

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
} from "./schema.ts"

export const workforceIpcRoutes = defineIpcRoutes({
  workforce: http.resource("workforce", {
    start: http.post("start", {
      body: StartWorkforceRequest,
      response: $type<StartWorkforceResponse>(),
    }),
    discoverCandidates: http.post("discover-candidates", {
      body: DiscoverWorkforceCandidatesRequest,
      response: $type<DiscoverWorkforceCandidatesResponse>(),
    }),
    initialize: http.post("initialize", {
      body: InitializeWorkforceRequest,
      response: $type<InitializeWorkforceResponse>(),
    }),
    get: http.post("get", {
      body: GetWorkforceRequest,
      response: $type<GetWorkforceResponse>(),
    }),
    list: http.get("list", {
      response: $type<ListWorkforcesResponse>(),
    }),
    shutdown: http.post("shutdown", {
      body: ShutdownWorkforceRequest,
      response: $type<ShutdownWorkforceResponse>(),
    }),
    request: http.post("request", {
      body: CreateWorkforceRequest,
      response: $type<MutateWorkforceResponse>(),
    }),
    update: http.post("update", {
      body: UpdateWorkforceRequest,
      response: $type<MutateWorkforceResponse>(),
    }),
    cancel: http.post("cancel", {
      body: CancelWorkforceRequest,
      response: $type<MutateWorkforceResponse>(),
    }),
    truncate: http.post("truncate", {
      body: TruncateWorkforceRequest,
      response: $type<MutateWorkforceResponse>(),
    }),
    respond: http.post("respond", {
      body: RespondWorkforceRequest,
      response: $type<MutateWorkforceResponse>(),
    }),
    suspend: http.post("suspend", {
      body: SuspendWorkforceRequest,
      response: $type<MutateWorkforceResponse>(),
    }),
    event: http.get("event", {
      query: SubscribeWorkforceEventsRequest,
      response: ndjson.$type<WorkforceEventEnvelope>(),
    }),
  }),
})
