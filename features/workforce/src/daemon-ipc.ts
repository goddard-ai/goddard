import { $type, defineIpcRoutes, http, ndjson } from "@goddard-ai/ipc"
import type { GetSessionWorkforceResponse } from "@goddard-ai/schema/daemon/sessions"
import { DaemonSessionIdParams } from "@goddard-ai/schema/id"

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
  session: http.resource("session", {
    workforce: http.resource("workforce", {
      /** Reads persisted workforce metadata attached to one daemon-managed session. */
      get: http.post("get", {
        body: DaemonSessionIdParams,
        response: $type<GetSessionWorkforceResponse>(),
      }),
    }),
  }),
  workforce: http.resource("workforce", {
    /** Starts or reuses one daemon workforce runtime. */
    start: http.post("start", {
      body: StartWorkforceRequest,
      response: $type<StartWorkforceResponse>(),
    }),
    /** Discovers package candidates for one repository workforce initialization flow. */
    discoverCandidates: http.post("discover-candidates", {
      body: DiscoverWorkforceCandidatesRequest,
      response: $type<DiscoverWorkforceCandidatesResponse>(),
    }),
    /** Initializes one repository workforce config and ledger through the daemon. */
    initialize: http.post("initialize", {
      body: InitializeWorkforceRequest,
      response: $type<InitializeWorkforceResponse>(),
    }),
    /** Fetches one daemon workforce runtime and its resolved config. */
    get: http.post("get", {
      body: GetWorkforceRequest,
      response: $type<GetWorkforceResponse>(),
    }),
    /** Lists daemon workforce runtime summaries. */
    list: http.get("list", {
      response: $type<ListWorkforcesResponse>(),
    }),
    /** Shuts down one daemon workforce runtime and reports whether shutdown succeeded. */
    shutdown: http.post("shutdown", {
      body: ShutdownWorkforceRequest,
      response: $type<ShutdownWorkforceResponse>(),
    }),
    /** Enqueues one workforce request and includes the updated workforce projection. */
    request: http.post("request", {
      body: CreateWorkforceRequest,
      response: $type<MutateWorkforceResponse>(),
    }),
    /** Updates one workforce request and includes the updated workforce projection. */
    update: http.post("update", {
      body: UpdateWorkforceRequest,
      response: $type<MutateWorkforceResponse>(),
    }),
    /** Cancels one workforce request and includes the updated workforce projection. */
    cancel: http.post("cancel", {
      body: CancelWorkforceRequest,
      response: $type<MutateWorkforceResponse>(),
    }),
    /** Truncates one workforce queue and includes the updated workforce projection. */
    truncate: http.post("truncate", {
      body: TruncateWorkforceRequest,
      response: $type<MutateWorkforceResponse>(),
    }),
    /** Responds to one active workforce request and includes the updated workforce projection. */
    respond: http.post("respond", {
      body: RespondWorkforceRequest,
      response: $type<MutateWorkforceResponse>(),
    }),
    /** Suspends one active workforce request and includes the updated workforce projection. */
    suspend: http.post("suspend", {
      body: SuspendWorkforceRequest,
      response: $type<MutateWorkforceResponse>(),
    }),
    /** Emits live daemon-published workforce ledger events for one repository root. */
    event: http.get("event", {
      query: SubscribeWorkforceEventsRequest,
      response: ndjson.$type<WorkforceEventEnvelope>(),
    }),
  }),
})
