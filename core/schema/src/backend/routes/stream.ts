import { BearerHeaders } from "@goddard-ai/auth/schema"
import { route } from "rouzer"

/** Opens the authenticated user-scoped feedback stream. */
export const repoStreamRoute = route("stream", {
  GET: {
    headers: BearerHeaders,
  },
})
