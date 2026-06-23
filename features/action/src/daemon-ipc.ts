import { $type, defineIpcRoutes, http, ipcMetadata } from "@goddard-ai/ipc"
import type { CreateSessionResponse } from "@goddard-ai/session/schema"

import { RunNamedActionRequest } from "./schema.ts"

export const actionIpcRoutes = defineIpcRoutes({
  action: http.resource("action", {
    ...ipcMetadata({
      description: "Named daemon action execution.",
    }),
    /** Runs one named daemon action and creates the resulting daemon session. */
    run: http.post("run", {
      ...ipcMetadata({
        description: "Runs one named daemon action and creates the resulting daemon session.",
      }),
      body: RunNamedActionRequest,
      response: $type<CreateSessionResponse>(),
    }),
  }),
})
