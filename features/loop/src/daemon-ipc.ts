import { $type, defineIpcRoutes, http, ipcMetadata } from "@goddard-ai/ipc"

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
    ...ipcMetadata({
      description: "Loop runtime control.",
    }),
    /** Starts or reuses one loop runtime. */
    start: http.post("start", {
      ...ipcMetadata({
        description: "Starts or reuses one loop runtime.",
      }),
      body: StartLoopRequest,
      response: $type<StartLoopResponse>(),
    }),
    /** Fetches one loop runtime and its resolved config. */
    get: http.post("get", {
      ...ipcMetadata({
        description: "Fetches one loop runtime and its resolved config.",
      }),
      body: GetLoopRequest,
      response: $type<GetLoopResponse>(),
    }),
    /** Lists loop runtime summaries. */
    list: http.get("list", {
      ...ipcMetadata({
        description: "Lists loop runtime summaries.",
      }),
      response: $type<ListLoopsResponse>(),
    }),
    /** Shuts down one loop and reports whether shutdown succeeded. */
    shutdown: http.post("shutdown", {
      ...ipcMetadata({
        description: "Shuts down one loop and reports whether shutdown succeeded.",
      }),
      body: ShutdownLoopRequest,
      response: $type<ShutdownLoopResponse>(),
    }),
  }),
})
