import { defineSdkPlugin } from "@goddard-ai/sdk-plugin"

import { loopIpcRoutes } from "./daemon-ipc.ts"
import type { GetLoopRequest, ShutdownLoopRequest, StartLoopRequest } from "./schema.ts"

export const loopSdkPlugin = defineSdkPlugin({
  name: "loop",
  ipcRoutes: loopIpcRoutes,
  wrap({ client }) {
    return {
      loop: {
        /** Starts or reuses one daemon loop runtime. */
        start: (input: StartLoopRequest) => client.loop.start(input),

        /** Fetches one daemon loop runtime and its resolved config. */
        get: (input: GetLoopRequest) => client.loop.get(input),

        /** Lists daemon loop runtime summaries. */
        list: () => client.loop.list(),

        /** Shuts down one daemon loop and reports whether shutdown succeeded. */
        shutdown: (input: ShutdownLoopRequest) => client.loop.shutdown(input),
      },
    }
  },
})
