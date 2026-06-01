import { BearerHeaders } from "@goddard-ai/auth/schema"
import { defineBackendRoutes, http } from "@goddard-ai/backend-plugin"

/** Remote-repo-owned backend routes. */
export const remoteRepoBackendRoutes = defineBackendRoutes({
  remoteRepo: http.resource("remote-repo", {
    stream: http.get("stream", {
      headers: BearerHeaders,
    }),
  }),
})
