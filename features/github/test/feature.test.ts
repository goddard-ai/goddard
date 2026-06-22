import { describe, expect, test } from "bun:test"

import {
  canGitHubPrincipalAccessRepository,
  githubBackendEvents,
  githubBackendRoutes,
  normalizeGitHubWebhookDelivery,
} from "../src/backend.ts"
import { GitHubWebhookDeliveryInput } from "../src/schema.ts"

describe("github feature package", () => {
  test("exports selected feature entrypoints", () => {
    expect(githubBackendRoutes).toEqual({})
    expect(Object.keys(githubBackendEvents)).toEqual(["remote_repo.event.received"])
  })

  test("normalizes GitHub webhook deliveries with provider provenance", () => {
    const delivery = GitHubWebhookDeliveryInput.parse({
      deliveryId: "delivery-1",
      receivedAt: "2026-01-01T00:00:00.000Z",
      event: {
        type: "issue_comment",
        owner: "goddard-ai",
        repo: "core",
        prNumber: 12,
        author: "alec",
        body: "please update",
      },
    })

    expect(normalizeGitHubWebhookDelivery(delivery)).toEqual({
      name: "remote_repo.event.received",
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

  test("authorizes GitHub principals by repository access", () => {
    const principal = {
      kind: "github_user" as const,
      githubUserId: 42,
      githubLogin: "alec",
      repositories: [{ owner: "goddard-ai", repo: "core" }],
    }

    expect(
      canGitHubPrincipalAccessRepository(principal, {
        owner: "goddard-ai",
        repo: "core",
      }),
    ).toBe(true)
    expect(
      canGitHubPrincipalAccessRepository(principal, {
        owner: "goddard-ai",
        repo: "other",
      }),
    ).toBe(false)
  })
})
