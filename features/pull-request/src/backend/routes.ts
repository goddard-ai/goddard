import { BearerHeaders } from "@goddard-ai/auth/schema"
import { $type, defineBackendRoutes, http, metadata } from "@goddard-ai/backend-plugin"

import { CreatePrInput, ManagedPrQuery, ReplyPrInput, type PullRequestRecord } from "../schema.ts"

/** Pull-request-owned backend routes grouped by PR domain action. */
export const pullRequestBackendRoutes = defineBackendRoutes({
  pullRequests: http.resource("pull-requests", {
    ...metadata({
      description: "Backend pull request records and comments.",
    }),
    create: http.post("create", {
      ...metadata({
        description: "Creates one backend pull request record.",
      }),
      headers: BearerHeaders,
      body: CreatePrInput,
      response: $type<PullRequestRecord>(),
    }),
    managed: http.get("managed", {
      ...metadata({
        description: "Checks whether one pull request is backend-managed.",
      }),
      headers: BearerHeaders,
      query: ManagedPrQuery,
      response: $type<{ managed: boolean }>(),
    }),
    comments: http.resource("comments", {
      ...metadata({
        description: "Backend pull request comment operations.",
      }),
      create: http.post("create", {
        ...metadata({
          description: "Creates one backend pull request comment.",
        }),
        headers: BearerHeaders,
        body: ReplyPrInput,
        response: $type<{ success: boolean }>(),
      }),
    }),
  }),
})
