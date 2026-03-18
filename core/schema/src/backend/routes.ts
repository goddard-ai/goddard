import { $type, route } from "rouzer"
import * as z from "zod/mini"
import type {
  AuthSession,
  CreatePrInput,
  DeviceFlowComplete,
  DeviceFlowSession,
  DeviceFlowStart,
  GitHubWebhookInput,
  GitHubWebhookReceipt,
  PullRequestRecord,
  RepoEvent,
} from "../backend.js"

const bearerHeaderSchema = z.object({
  authorization: z.string(),
})

const githubWebhookHeaderSchema = z.object({
  "x-github-delivery": z.string(),
  "x-github-event": z.string(),
  "x-hub-signature-256": z.string(),
})

export const authDeviceStartRoute = route("auth/device/start", {
  POST: {
    body: z.object({
      githubUsername: z.optional(z.string()),
    }),
    response: $type<DeviceFlowSession>(),
  },
})

export const authDeviceCompleteRoute = route("auth/device/complete", {
  POST: {
    body: z.object({
      deviceCode: z.string(),
      githubUsername: z.string(),
    }),
    response: $type<AuthSession>(),
  },
})

export const authSessionRoute = route("auth/session", {
  GET: {
    headers: bearerHeaderSchema,
    response: $type<AuthSession>(),
  },
})

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

export const githubWebhookRoute = route("webhooks/github", {
  POST: {
    headers: githubWebhookHeaderSchema,
    response: $type<GitHubWebhookReceipt>(),
  },
})

export const githubWebhookEventRoute = route("webhooks/github/events", {
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

export const repoStreamRoute = route("stream", {
  GET: {
    headers: bearerHeaderSchema,
    query: z.object({
      owner: z.string(),
      repo: z.string(),
    }),
  },
})

export type {
  CreatePrInput,
  DeviceFlowComplete,
  DeviceFlowStart,
  GitHubWebhookInput,
  GitHubWebhookReceipt,
  ReplyPrInput,
} from "../backend.js"
