import {
  REMOTE_REPO_PULL_REQUEST_COMMENT_CREATED,
  REMOTE_REPO_PULL_REQUEST_REVIEW_SUBMITTED,
} from "@goddard-ai/remote-repo/backend"
import { describe, expect, test } from "bun:test"

import {
  githubBackendPlugin,
  githubBackendRoutes,
  normalizeGitHubWebhookRequest,
} from "../src/backend.ts"

describe("github feature package", () => {
  test("exports selected feature entrypoints", () => {
    expect(githubBackendRoutes.webhooks.path.source).toBe("/webhooks")
    expect(githubBackendRoutes.webhooks.children.github.path?.source).toBe("/github")
    expect(githubBackendPlugin.name).toBe("github")
    expect(githubBackendPlugin.routes?.webhooks.children.github.path?.source).toBe("/github")
    expect("events" in githubBackendPlugin).toBe(false)
    expect("eventSources" in githubBackendPlugin).toBe(false)
  })

  test("normalizes raw GitHub webhook deliveries with provider provenance", () => {
    expect(
      normalizeGitHubWebhookRequest({
        deliveryId: "delivery-1",
        receivedAt: "2026-01-01T00:00:00.000Z",
        eventName: "issue_comment",
        payload: {
          action: "created",
          issue: {
            number: 12,
            pull_request: {},
          },
          comment: {
            user: { login: "alec", type: "User" },
            body: "please update",
          },
          repository: {
            name: "core",
            owner: { login: "goddard-ai" },
          },
          sender: { login: "alec", type: "User" },
        },
      }),
    ).toEqual({
      name: REMOTE_REPO_PULL_REQUEST_COMMENT_CREATED,
      payload: {
        type: "comment",
        owner: "goddard-ai",
        repo: "core",
        prNumber: 12,
        author: "alec",
        body: "please update",
        reactionAdded: "eyes",
        createdAt: "2026-01-01T00:00:00.000Z",
      },
      provenance: {
        provider: "github",
        deliveryId: "delivery-1",
        webhookType: "issue_comment",
      },
    })
  })

  test("ignores unsupported and bot-originated webhooks", () => {
    expect(
      normalizeGitHubWebhookRequest({
        deliveryId: "delivery-1",
        eventName: "pull_request",
        payload: {},
      }),
    ).toBeUndefined()
    expect(
      normalizeGitHubWebhookRequest({
        deliveryId: "delivery-2",
        eventName: "issue_comment",
        payload: {
          action: "created",
          issue: {
            number: 12,
            pull_request: {},
          },
          comment: {
            user: { login: "bot", type: "Bot" },
            body: "please update",
          },
          repository: {
            name: "core",
            owner: { login: "goddard-ai" },
          },
          sender: { login: "bot", type: "Bot" },
        },
      }),
    ).toBeUndefined()
  })

  test("normalizes pull request review webhooks", () => {
    expect(
      normalizeGitHubWebhookRequest({
        deliveryId: "delivery-1",
        receivedAt: "2026-01-01T00:00:00.000Z",
        eventName: "pull_request_review",
        payload: {
          action: "submitted",
          pull_request: {
            number: 12,
          },
          review: {
            user: { login: "reviewer", type: "User" },
            state: "approved",
            body: "ship it",
          },
          repository: {
            name: "core",
            owner: { login: "goddard-ai" },
          },
          sender: { login: "reviewer", type: "User" },
        },
      }),
    ).toEqual({
      name: REMOTE_REPO_PULL_REQUEST_REVIEW_SUBMITTED,
      payload: {
        type: "review",
        owner: "goddard-ai",
        repo: "core",
        prNumber: 12,
        author: "reviewer",
        state: "approved",
        body: "ship it",
        reactionAdded: "eyes",
        createdAt: "2026-01-01T00:00:00.000Z",
      },
      provenance: {
        provider: "github",
        deliveryId: "delivery-1",
        webhookType: "pull_request_review",
      },
    })
  })
})
