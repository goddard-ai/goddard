import { BearerHeaders } from "@goddard-ai/auth/schema"
import { $type, defineBackendRoutes, http, metadata } from "@goddard-ai/backend-plugin"

import type { RepoEvent } from "../schema.ts"

/** Remote-repo-owned backend routes. */
export const remoteRepoBackendRoutes = defineBackendRoutes({
  remoteRepo: http.resource("remote-repo", {
    ...metadata({
      description: "Backend remote repository event streaming.",
    }),
    stream: http.get("stream", {
      ...metadata({
        description: "Streams backend remote repository events.",
      }),
      headers: BearerHeaders,
    }),
  }),
  webhooks: http.resource("webhooks", {
    ...metadata({
      description: "Backend webhook ingestion.",
    }),
    github: http.post("github", {
      ...metadata({
        description: "Ingests one GitHub webhook payload.",
      }),
      body: http.rawBody(),
      response: $type<RepoEvent>(),
    }),
  }),
})
