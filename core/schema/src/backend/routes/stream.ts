import { route } from "rouzer"
import { BearerHeaders } from "../../common/auth.js"

/** Opens the authenticated user-scoped feedback stream. */
export const repoStreamRoute = route("stream", {
  GET: {
    headers: BearerHeaders,
  },
})
