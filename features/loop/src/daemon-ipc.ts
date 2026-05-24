import { $type, defineIpcRoutes, http } from "@goddard-ai/ipc"

import {
  GetLoopRequest,
  ShutdownLoopRequest,
  StartLoopRequest,
  type GetLoopResponse,
  type ListLoopsResponse,
  type ShutdownLoopResponse,
  type StartLoopResponse,
} from "./schema.ts"

export const loopIpcRoutes = defineIpcRoutes({
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
})
