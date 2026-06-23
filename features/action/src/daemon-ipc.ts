import { $type, defineIpcRoutes, http, ipcMetadata } from "@goddard-ai/ipc"
import type { CreateSessionResponse } from "@goddard-ai/session/schema"

import { RunNamedActionRequest } from "./schema.ts"

export const actionIpcRoutes = defineIpcRoutes({
  action: http.resource("action", {
    ...ipcMetadata({
      description: "Named action execution.",
    }),
    /** Runs one named action and creates the resulting session. */
    run: http.post("run", {
      ...ipcMetadata({
        description: "Runs one named action and creates the resulting session.",
      }),
      body: RunNamedActionRequest,
      response: $type<CreateSessionResponse>(),
    }),
  }),
})
