import { $type, defineIpcRoutes, http } from "@goddard-ai/ipc"
import type { CreateSessionResponse } from "@goddard-ai/session/schema"

import { RunNamedActionRequest } from "./schema.ts"

export const actionIpcRoutes = defineIpcRoutes({
  action: http.resource("action", {
    /** Runs one named daemon action and creates the resulting daemon session. */
    run: http.post("run", {
      body: RunNamedActionRequest,
      response: $type<CreateSessionResponse>(),
    }),
  }),
})
