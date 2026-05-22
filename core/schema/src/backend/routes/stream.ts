import { BearerHeaders } from "@goddard-ai/auth/schema"
import * as http from "rouzer/http"

/** Repository-scoped backend routes owned by the core backend substrate. */
export const repositories = http.resource("repositories", {
  stream: http.get("stream", {
    headers: BearerHeaders,
  }),
})
