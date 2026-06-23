import {
  REMOTE_REPO_PULL_REQUEST_COMMENT_CREATED,
  REMOTE_REPO_PULL_REQUEST_REVIEW_SUBMITTED,
} from "@goddard-ai/remote-repo/backend"
import { describe, expect, test } from "bun:test"

import {
  githubBackendPlugin,
  githubBackendRoutes,
  normalizeGitHubWebhookRequest,
  readGitHubWebhookRequest,
  signGitHubWebhookBody,
} from "../src/backend.ts"
import { canGitHubPrincipalAccessRepository, createGitHubBackendPrincipal } from "../src/schema.ts"

describe("github feature package", () => {
  test("exports selected feature entrypoints", () => {
    expect(githubBackendRoutes.webhooks.path.source).toBe("/webhooks")
    expect(githubBackendRoutes.webhooks.children.github.path?.source).toBe("/github")
    expect(githubBackendPlugin.name).toBe("github")
    expect(githubBackendPlugin.routes?.webhooks.children.github.path?.source).toBe("/github")
    expect("events" in githubBackendPlugin).toBe(false)
    expect("eventSources" in githubBackendPlugin).toBe(false)
  })

  test("creates provider-neutral principals from GitHub identity", () => {
    expect(
      createGitHubBackendPrincipal({
        provider: "github",
        githubUserId: 42,
        githubLogin: "alec",
      }),
    ).toEqual({
      id: "github:42",
      providerIdentities: [
        {
          provider: "github",
          subject: "42",
          displayName: "alec",
        },
      ],
    })
  })

  test("authorizes GitHub repository grants", () => {
    const grants = {
      identity: {
        provider: "github" as const,
        githubUserId: 42,
        githubLogin: "alec",
      },
      repositories: [{ owner: "goddard-ai", repo: "core" }],
    }

    expect(
      canGitHubPrincipalAccessRepository(grants, {
        owner: "goddard-ai",
        repo: "core",
      }),
    ).toBe(true)
    expect(
      canGitHubPrincipalAccessRepository(grants, {
        owner: "goddard-ai",
        repo: "other",
      }),
    ).toBe(false)
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

  test("rejects GitHub webhook requests with invalid configured signatures", async () => {
    const request = new Request("https://example.test/webhooks/github", {
      method: "POST",
      headers: {
        "content-type": "application/json",
        "x-hub-signature-256": "sha256=bad",
      },
      body: JSON.stringify({ action: "created" }),
    })

    await expect(readGitHubWebhookRequest(request, "secret")).rejects.toMatchObject({
      statusCode: 401,
      message: "Invalid GitHub webhook signature",
    })
  })

  test("accepts GitHub webhook requests with valid configured signatures", async () => {
    const body = JSON.stringify({ action: "created" })
    const request = new Request("https://example.test/webhooks/github", {
      method: "POST",
      headers: {
        "content-type": "application/json",
        "x-github-event": "issue_comment",
        "x-github-delivery": "delivery-1",
        "x-hub-signature-256": await signGitHubWebhookBody("secret", body),
      },
      body,
    })

    await expect(readGitHubWebhookRequest(request, "secret")).resolves.toEqual({
      deliveryId: "delivery-1",
      eventName: "issue_comment",
      payload: { action: "created" },
    })
  })
})
