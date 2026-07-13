import { $type, defineIpcRoutes, http, metadata, ndjson } from "@goddard-ai/ipc"

import {
  InspectVscodeTasksRequest,
  ResolveVscodeTaskRequest,
  VscodeTaskCancelRequest,
  VscodeTaskConnectionParams,
  VscodeTaskConnectRequest,
  VscodeTaskRunRequest,
  type InspectVscodeTasksResponse,
  type ResolveVscodeTaskResponse,
  type VscodeTaskConnectResponse,
  type VscodeTaskDaemonEvent,
  type VscodeTaskMutationResponse,
  type VscodeTaskRunResponse,
} from "./schema.ts"

export const vscodeTaskIpcRoutes = defineIpcRoutes({
  vscodeTask: http.resource("vscode-task", {
    ...metadata({
      description: "Inspects, resolves, and runs workspace VS Code tasks.",
    }),
    inspect: http.post("inspect", {
      ...metadata({ description: "Inspects the supported tasks in a workspace." }),
      body: InspectVscodeTasksRequest,
      response: $type<InspectVscodeTasksResponse>(),
    }),
    resolve: http.post("resolve", {
      ...metadata({ description: "Resolves a workspace task into an execution preview." }),
      body: ResolveVscodeTaskRequest,
      response: $type<ResolveVscodeTaskResponse>(),
    }),
    connect: http.post("connect", {
      ...metadata({ description: "Opens a connection-scoped workspace-task stream." }),
      body: VscodeTaskConnectRequest,
      response: $type<VscodeTaskConnectResponse>(),
    }),
    run: http.post("run", {
      ...metadata({ description: "Starts a workspace task on an existing connection." }),
      body: VscodeTaskRunRequest,
      response: $type<VscodeTaskRunResponse>(),
    }),
    cancel: http.post("cancel", {
      ...metadata({ description: "Cancels an active workspace-task run." }),
      body: VscodeTaskCancelRequest,
      response: $type<VscodeTaskMutationResponse>(),
    }),
    disconnect: http.post("disconnect", {
      ...metadata({ description: "Disposes a workspace-task connection and its runs." }),
      body: VscodeTaskConnectionParams,
      response: $type<VscodeTaskMutationResponse>(),
    }),
    event: http.get("events", {
      ...metadata({ description: "Streams lifecycle and PTY output for one connection." }),
      query: VscodeTaskConnectionParams,
      response: ndjson.$type<VscodeTaskDaemonEvent>(),
    }),
  }),
})
