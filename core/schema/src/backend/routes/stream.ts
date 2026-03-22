import { route } from "rouzer"
import { BearerHeaders } from "../../common/auth.ts"

/** Opens the authenticated user-scoped feedback stream. */
export const repoStreamRoute = route("stream", {
  GET: {
    headers: BearerHeaders,
  },
})
