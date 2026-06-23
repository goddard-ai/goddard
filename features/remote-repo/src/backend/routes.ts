import { BearerHeaders } from "@goddard-ai/auth/schema"
import { $type, defineBackendRoutes, http, metadata, ndjson } from "@goddard-ai/backend-plugin"

import { BackendEventStreamRequest, type RepoEvent } from "../schema.ts"

/** Remote-repo-owned backend routes. */
export const remoteRepoBackendRoutes = defineBackendRoutes({
  events: http.resource("events", {
    ...metadata({
      description: "Backend remote repository event streaming.",
    }),
    stream: http.post("stream", {
      ...metadata({
        description: "Streams backend remote repository events.",
      }),
      headers: BearerHeaders,
      body: BackendEventStreamRequest,
      response: ndjson.$type<RepoEvent>(),
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
