import * as http from "rouzer/http"
import { z } from "zod"

const AuthenticatedBackendHeaders = z.object({
  authorization: z.string(),
})

/** Repository-scoped backend routes owned by the core backend substrate. */
export const repositories = http.resource("repositories", {
  stream: http.get("stream", {
    headers: AuthenticatedBackendHeaders,
  }),
})
