import { $type, defineIpcRoutes, http, metadata } from "@goddard-ai/ipc"
import type { CreateSessionResponse } from "@goddard-ai/session/schema"

import { RunNamedActionRequest } from "./schema.ts"

export const actionIpcRoutes = defineIpcRoutes({
  action: http.resource("action", {
    ...metadata({
      description: "Named action execution.",
    }),
    /** Runs one named action and creates the resulting session. */
    run: http.post("run", {
      ...metadata({
        description: "Runs one named action and creates the resulting session.",
      }),
      body: RunNamedActionRequest,
      response: $type<CreateSessionResponse>(),
    }),
  }),
})
