import { $type, defineIpcRoutes, http } from "@goddard-ai/ipc"
import type { CreateSessionResponse } from "@goddard-ai/schema/daemon/sessions"

import { RunNamedActionRequest } from "./schema.ts"

export const actionIpcRoutes = defineIpcRoutes({
  action: http.resource("action", {
    run: http.post("run", {
      body: RunNamedActionRequest,
      response: $type<CreateSessionResponse>(),
    }),
  }),
})
