import { $type, defineIpcRoutes, http, ipcMetadata, ndjson } from "@goddard-ai/ipc"
import { SessionIdParams } from "@goddard-ai/session/schema"

import {
  CancelWorkforceRequest,
  CreateWorkforceRequest,
  DiscoverWorkforceCandidatesRequest,
  GetWorkforceRequest,
  InitializeWorkforceRequest,
  RespondWorkforceRequest,
  ShutdownWorkforceRequest,
  StartWorkforceRequest,
  SuspendWorkforceRequest,
  TruncateWorkforceRequest,
  UpdateWorkforceRequest,
  type DiscoverWorkforceCandidatesResponse,
  type GetSessionWorkforceResponse,
  type GetWorkforceResponse,
  type InitializeWorkforceResponse,
  type ListWorkforcesResponse,
  type MutateWorkforceResponse,
  type ShutdownWorkforceResponse,
  type StartWorkforceResponse,
} from "./schema.ts"

export const workforceIpcRoutes = defineIpcRoutes({
  session: http.resource("session", {
    ...ipcMetadata({
      description: "Daemon-managed session extensions.",
    }),
    workforce: http.resource("workforce", {
      ...ipcMetadata({
        description: "Session-attached workforce metadata.",
      }),
      /** Reads persisted workforce metadata attached to one daemon-managed session. */
      get: http.post("get", {
        ...ipcMetadata({
          description: "Reads persisted workforce metadata attached to one daemon-managed session.",
        }),
        body: SessionIdParams,
        response: $type<GetSessionWorkforceResponse>(),
      }),
    }),
  }),
  workforce: http.resource("workforce", {
    ...ipcMetadata({
      description: "Daemon workforce runtime control.",
    }),
    /** Starts or reuses one daemon workforce runtime. */
    start: http.post("start", {
      ...ipcMetadata({
        description: "Starts or reuses one daemon workforce runtime.",
      }),
      body: StartWorkforceRequest,
      response: $type<StartWorkforceResponse>(),
    }),
    /** Discovers package candidates for one repository workforce initialization flow. */
    discoverCandidates: http.post("discover-candidates", {
      ...ipcMetadata({
        description:
          "Discovers package candidates for one repository workforce initialization flow.",
      }),
      body: DiscoverWorkforceCandidatesRequest,
      response: $type<DiscoverWorkforceCandidatesResponse>(),
    }),
    /** Initializes one repository workforce config and ledger through the daemon. */
    initialize: http.post("initialize", {
      ...ipcMetadata({
        description: "Initializes one repository workforce config and ledger through the daemon.",
      }),
      body: InitializeWorkforceRequest,
      response: $type<InitializeWorkforceResponse>(),
    }),
    /** Fetches one daemon workforce runtime and its resolved config. */
    get: http.post("get", {
      ...ipcMetadata({
        description: "Fetches one daemon workforce runtime and its resolved config.",
      }),
      body: GetWorkforceRequest,
      response: $type<GetWorkforceResponse>(),
    }),
    /** Lists daemon workforce runtime summaries. */
    list: http.get("list", {
      ...ipcMetadata({
        description: "Lists daemon workforce runtime summaries.",
      }),
      response: $type<ListWorkforcesResponse>(),
    }),
    /** Shuts down one daemon workforce runtime and reports whether shutdown succeeded. */
    shutdown: http.post("shutdown", {
      ...ipcMetadata({
        description:
          "Shuts down one daemon workforce runtime and reports whether shutdown succeeded.",
      }),
      body: ShutdownWorkforceRequest,
      response: $type<ShutdownWorkforceResponse>(),
    }),
    /** Enqueues one workforce request and includes the updated workforce projection. */
    request: http.post("request", {
      ...ipcMetadata({
        description:
          "Enqueues one workforce request and includes the updated workforce projection.",
      }),
      body: CreateWorkforceRequest,
      response: $type<MutateWorkforceResponse>(),
    }),
    /** Updates one workforce request and includes the updated workforce projection. */
    update: http.post("update", {
      ...ipcMetadata({
        description: "Updates one workforce request and includes the updated workforce projection.",
      }),
      body: UpdateWorkforceRequest,
      response: $type<MutateWorkforceResponse>(),
    }),
    /** Cancels one workforce request and includes the updated workforce projection. */
    cancel: http.post("cancel", {
      ...ipcMetadata({
        description: "Cancels one workforce request and includes the updated workforce projection.",
      }),
      body: CancelWorkforceRequest,
      response: $type<MutateWorkforceResponse>(),
    }),
    /** Truncates one workforce queue and includes the updated workforce projection. */
    truncate: http.post("truncate", {
      ...ipcMetadata({
        description: "Truncates one workforce queue and includes the updated workforce projection.",
      }),
      body: TruncateWorkforceRequest,
      response: $type<MutateWorkforceResponse>(),
    }),
    /** Responds to one active workforce request and includes the updated workforce projection. */
    respond: http.post("respond", {
      ...ipcMetadata({
        description:
          "Responds to one active workforce request and includes the updated workforce projection.",
      }),
      body: RespondWorkforceRequest,
      response: $type<MutateWorkforceResponse>(),
    }),
    /** Suspends one active workforce request and includes the updated workforce projection. */
    suspend: http.post("suspend", {
      ...ipcMetadata({
        description:
          "Suspends one active workforce request and includes the updated workforce projection.",
      }),
      body: SuspendWorkforceRequest,
      response: $type<MutateWorkforceResponse>(),
    }),
    /** Streams live daemon-published workforce ledger events for one repository root. */
    streamEvents: http.get("stream-events", {
      ...ipcMetadata({
        description:
          "Streams live daemon-published workforce ledger events for one repository root.",
      }),
      query: SubscribeWorkforceEventsRequest,
      response: ndjson.$type<WorkforceLedgerEvent>(),
    }),
  }),
})
