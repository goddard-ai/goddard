import { BearerHeaders } from "@goddard-ai/auth/schema"
import { $type, backendMetadata, defineBackendRoutes, http } from "@goddard-ai/backend-plugin"

import type { RepoEvent } from "../schema.ts"

/** Remote-repo-owned backend routes. */
export const remoteRepoBackendRoutes = defineBackendRoutes({
  remoteRepo: http.resource("remote-repo", {
    ...backendMetadata({
      description: "Backend remote repository event streaming.",
    }),
    stream: http.get("stream", {
      ...backendMetadata({
        description: "Streams backend remote repository events.",
      }),
      headers: BearerHeaders,
    }),
  }),
  webhooks: http.resource("webhooks", {
    ...backendMetadata({
      description: "Backend webhook ingestion.",
    }),
    github: http.post("github", {
      ...backendMetadata({
        description: "Ingests one GitHub webhook payload.",
      }),
      body: http.rawBody(),
      response: $type<RepoEvent>(),
    }),
  }),
})
