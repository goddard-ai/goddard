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
      description: "Daemon loop runtime control.",
    }),
    /** Starts or reuses one daemon loop runtime. */
    start: http.post("start", {
      ...ipcMetadata({
        description: "Starts or reuses one daemon loop runtime.",
      }),
      body: StartLoopRequest,
      response: $type<StartLoopResponse>(),
    }),
    /** Fetches one daemon loop runtime and its resolved config. */
    get: http.post("get", {
      ...ipcMetadata({
        description: "Fetches one daemon loop runtime and its resolved config.",
      }),
      body: GetLoopRequest,
      response: $type<GetLoopResponse>(),
    }),
    /** Lists daemon loop runtime summaries. */
    list: http.get("list", {
      ...ipcMetadata({
        description: "Lists daemon loop runtime summaries.",
      }),
      response: $type<ListLoopsResponse>(),
    }),
    /** Shuts down one daemon loop and reports whether shutdown succeeded. */
    shutdown: http.post("shutdown", {
      ...ipcMetadata({
        description: "Shuts down one daemon loop and reports whether shutdown succeeded.",
      }),
      body: ShutdownLoopRequest,
      response: $type<ShutdownLoopResponse>(),
    }),
  }),
})
