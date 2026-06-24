import { BearerHeaders } from "@goddard-ai/auth/schema"
import { defineBackendRoutes, http, metadata, ndjson } from "@goddard-ai/backend-plugin"

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
})
