import { $type, route } from "rouzer"
import * as z from "zod/mini"
import type { AuthSession, DeviceFlowSession, PullRequestRecord, RepoEvent } from "../backend.js"

const bearerHeaderSchema = z.object({
  authorization: z.string(),
})

/** Starts the GitHub device flow for a pending user session. */
export const authDeviceStartRoute = route("auth/device/start", {
  POST: {
    body: z.object({
      githubUsername: z.optional(z.string()),
    }),
    response: $type<DeviceFlowSession>(),
  },
})

/** Completes the GitHub device flow and returns an authenticated backend session. */
export const authDeviceCompleteRoute = route("auth/device/complete", {
  POST: {
    body: z.object({
      deviceCode: z.string(),
      githubUsername: z.string(),
    }),
    response: $type<AuthSession>(),
  },
})

/** Reads the current authenticated backend session. */
export const authSessionRoute = route("auth/session", {
  GET: {
    headers: bearerHeaderSchema,
    response: $type<AuthSession>(),
  },
})

/** Creates a managed pull request through the backend. */
export const prCreateRoute = route("pr/create", {
  POST: {
    headers: bearerHeaderSchema,
    body: z.object({
      owner: z.string(),
      repo: z.string(),
      title: z.string(),
      body: z.optional(z.string()),
      head: z.string(),
      base: z.string(),
    }),
    response: $type<PullRequestRecord>(),
  },
})

/** Posts a managed pull-request reply through the backend. */
export const prReplyRoute = route("pr/reply", {
  POST: {
    headers: bearerHeaderSchema,
    body: z.object({
      owner: z.string(),
      repo: z.string(),
      prNumber: z.number(),
      body: z.string(),
    }),
    response: $type<{ success: boolean }>(),
  },
})

/** Reports whether a pull request is managed by the authenticated user. */
export const prManagedRoute = route("pr/managed", {
  GET: {
    headers: bearerHeaderSchema,
    query: z.object({
      owner: z.string(),
      repo: z.string(),
      prNumber: z.coerce.number(),
    }),
    response: $type<{ managed: boolean }>(),
  },
})

/** Receives normalized GitHub webhook payloads for managed PR feedback. */
export const githubWebhookRoute = route("webhooks/github", {
  POST: {
    body: z.union([
      z.object({
        type: z.literal("issue_comment"),
        owner: z.string(),
        repo: z.string(),
        prNumber: z.number(),
        author: z.string(),
        body: z.string(),
      }),
      z.object({
        type: z.literal("pull_request_review"),
        owner: z.string(),
        repo: z.string(),
        prNumber: z.number(),
        author: z.string(),
        state: z.enum(["approved", "changes_requested", "commented"]),
        body: z.string(),
      }),
    ]),
    response: $type<RepoEvent>(),
  },
})

/** Opens the authenticated user-scoped feedback stream. */
export const repoStreamRoute = route("stream", {
  GET: {
    headers: bearerHeaderSchema,
  },
})

export type {
  CreatePrInput,
  DeviceFlowComplete,
  DeviceFlowStart,
  GitHubWebhookInput,
  ReplyPrInput,
} from "../backend.js"
