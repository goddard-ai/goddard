import { BearerHeaders } from "@goddard-ai/auth/schema"
import * as http from "rouzer/http"

/** Opens the authenticated user-scoped feedback stream. */
export const repoStream = http.get("stream", {
  headers: BearerHeaders,
})
