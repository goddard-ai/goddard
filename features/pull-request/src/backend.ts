import { BearerHeaders } from "@goddard-ai/auth/schema"
import { $type } from "rouzer"
import * as http from "rouzer/http"

import {
  CreatePrInput,
  GitHubWebhookInput,
  ManagedPrQuery,
  ReplyPrInput,
  type PullRequestRecord,
  type RepoEvent,
} from "./schema.ts"

/** Creates a managed pull request through the backend. */
export const prCreate = http.post("pr/create", {
  headers: BearerHeaders,
  body: CreatePrInput,
  response: $type<PullRequestRecord>(),
})

/** Posts a managed pull-request reply through the backend. */
export const prReply = http.post("pr/reply", {
  headers: BearerHeaders,
  body: ReplyPrInput,
  response: $type<{ success: boolean }>(),
})

/** Reports whether a pull request is managed by the authenticated user. */
export const prManaged = http.get("pr/managed", {
  headers: BearerHeaders,
  query: ManagedPrQuery,
  response: $type<{ managed: boolean }>(),
})

/** Receives normalized GitHub webhook payloads for managed PR feedback. */
export const githubWebhook = http.post("webhooks/github", {
  body: GitHubWebhookInput,
  response: $type<RepoEvent>(),
})
