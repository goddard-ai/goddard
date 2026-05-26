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
    /** Starts or reuses one daemon loop runtime. */
    start: http.post("start", {
      body: StartLoopRequest,
      response: $type<StartLoopResponse>(),
    }),
    /** Fetches one daemon loop runtime and its resolved config. */
    get: http.post("get", {
      body: GetLoopRequest,
      response: $type<GetLoopResponse>(),
    }),
    /** Lists daemon loop runtime summaries. */
    list: http.get("list", {
      response: $type<ListLoopsResponse>(),
    }),
    /** Shuts down one daemon loop and reports whether shutdown succeeded. */
    shutdown: http.post("shutdown", {
      body: ShutdownLoopRequest,
      response: $type<ShutdownLoopResponse>(),
    }),
  }),
})
