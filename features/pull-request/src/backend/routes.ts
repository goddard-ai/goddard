import { BearerHeaders } from "@goddard-ai/auth/schema"
import { $type, defineBackendRoutes, http } from "@goddard-ai/backend-plugin"

import {
  CreatePrInput,
  GitHubWebhookInput,
  ManagedPrQuery,
  ReplyPrInput,
  type PullRequestRecord,
  type RepoEvent,
} from "../schema.ts"

/** Pull-request-owned backend routes grouped by PR domain action. */
export const pullRequestBackendRoutes = defineBackendRoutes({
  pullRequests: http.resource("pull-requests", {
    create: http.post("create", {
      headers: BearerHeaders,
      body: CreatePrInput,
      response: $type<PullRequestRecord>(),
    }),
    managed: http.get("managed", {
      headers: BearerHeaders,
      query: ManagedPrQuery,
      response: $type<{ managed: boolean }>(),
    }),
    comments: http.resource("comments", {
      create: http.post("create", {
        headers: BearerHeaders,
        body: ReplyPrInput,
        response: $type<{ success: boolean }>(),
      }),
    }),
  }),
  webhooks: http.resource("webhooks", {
    github: http.post("github", {
      body: GitHubWebhookInput,
      response: $type<RepoEvent>(),
    }),
  }),
})
