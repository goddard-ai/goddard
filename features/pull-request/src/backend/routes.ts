import { BearerHeaders } from "@goddard-ai/auth/schema"
import { $type, backendMetadata, defineBackendRoutes, http } from "@goddard-ai/backend-plugin"

import { CreatePrInput, ManagedPrQuery, ReplyPrInput, type PullRequestRecord } from "../schema.ts"

/** Pull-request-owned backend routes grouped by PR domain action. */
export const pullRequestBackendRoutes = defineBackendRoutes({
  pullRequests: http.resource("pull-requests", {
    ...backendMetadata({
      description: "Backend pull request records and comments.",
    }),
    create: http.post("create", {
      ...backendMetadata({
        description: "Creates one backend pull request record.",
      }),
      headers: BearerHeaders,
      body: CreatePrInput,
      response: $type<PullRequestRecord>(),
    }),
    managed: http.get("managed", {
      ...backendMetadata({
        description: "Checks whether one pull request is backend-managed.",
      }),
      headers: BearerHeaders,
      query: ManagedPrQuery,
      response: $type<{ managed: boolean }>(),
    }),
    comments: http.resource("comments", {
      ...backendMetadata({
        description: "Backend pull request comment operations.",
      }),
      create: http.post("create", {
        ...backendMetadata({
          description: "Creates one backend pull request comment.",
        }),
        headers: BearerHeaders,
        body: ReplyPrInput,
        response: $type<{ success: boolean }>(),
      }),
    }),
  }),
})
