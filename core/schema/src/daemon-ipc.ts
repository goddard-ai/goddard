import { actionIpcRoutes } from "@goddard-ai/action/daemon-ipc"
import { adapterIpcRoutes } from "@goddard-ai/adapter/daemon-ipc"
import { authIpcRoutes } from "@goddard-ai/auth/daemon-ipc"
import { inboxIpcRoutes } from "@goddard-ai/inbox/daemon-ipc"
import {
  $type,
  composeIpcRoutes,
  defineIpcRoutes,
  getResponsePluginMarkerId,
  http,
  ndjson,
  type HttpRouteTree,
} from "@goddard-ai/ipc"
import { pullRequestIpcRoutes } from "@goddard-ai/pull-request/daemon-ipc"
import { sessionIpcRoutes } from "@goddard-ai/session/daemon-ipc"

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

const coreDaemonIpcRoutes = defineIpcRoutes({
  daemon: http.resource("daemon", {
    health: http.get("health", {
      response: $type<{ ok: boolean }>(),
    }),
  }),
  loop: http.resource("loop", {
    start: http.post("start", {
      body: StartLoopRequest,
      response: $type<StartLoopResponse>(),
    }),
    get: http.post("get", {
      body: GetLoopRequest,
      response: $type<GetLoopResponse>(),
    }),
    list: http.get("list", {
      response: $type<ListLoopsResponse>(),
    }),
    shutdown: http.post("shutdown", {
      body: ShutdownLoopRequest,
      response: $type<ShutdownLoopResponse>(),
    }),
  }),
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

/** IPC route tree shared by daemon clients and server composition roots. */
export const daemonIpcRoutes = composeIpcRoutes([
  coreDaemonIpcRoutes,
  actionIpcRoutes,
  adapterIpcRoutes,
  authIpcRoutes,
  inboxIpcRoutes,
  pullRequestIpcRoutes,
  sessionIpcRoutes,
])

/** Compatibility schema for the old transport while the daemon server moves to Rouzer. */
export const daemonIpcSchema = createLegacySchemaFromRoutes(daemonIpcRoutes)

function createLegacySchemaFromRoutes(routes: HttpRouteTree) {
  const requests: Record<string, unknown> = {}
  const streams: Record<string, unknown> = {}

  function visit(node: unknown, path: string[]) {
    if (!node || typeof node !== "object") {
      return
    }

    if ("kind" in node && node.kind === "resource" && "children" in node) {
      for (const [key, child] of Object.entries(node.children as Record<string, unknown>)) {
        visit(child, [...path, key])
      }
      return
    }

    if ("kind" in node && node.kind === "action" && "schema" in node) {
      const schema = node.schema as {
        readonly body?: unknown
        readonly query?: unknown
        readonly response?: unknown
      }
      const name = path.join(".")
      if (schema.query && isNdjsonResponse(schema.response)) {
        streams[name] = {
          payload: schema.response,
          filter: schema.query,
        }
        return
      }

      requests[name] = {
        payload: schema.body,
        response: schema.response,
      }
    }
  }

  for (const [key, child] of Object.entries(routes)) {
    visit(child, [key])
  }

  return { requests, streams }
}

function isNdjsonResponse(response: unknown) {
  return getResponsePluginMarkerId(response) === "rouzer/ndjson"
}
