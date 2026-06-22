import { BearerHeaders } from "@goddard-ai/auth/schema"
import {
  $type,
  defineBackendRoutes,
  http,
  type BackendEventEnvelope,
} from "@goddard-ai/backend-plugin"

import type { RepoEvent } from "../schema.ts"

/** Remote-repo-owned backend routes. */
export const remoteRepoBackendRoutes = defineBackendRoutes({
  remoteRepo: http.resource("remote-repo", {
    stream: http.get("stream", {
      headers: BearerHeaders,
    }),
  }),
  webhooks: http.resource("webhooks", {
    github: http.post("github", {
      body: http.rawBody(),
      response: $type<BackendEventEnvelope<"remote_repo.event.received", RepoEvent>>(),
    }),
  }),
})
