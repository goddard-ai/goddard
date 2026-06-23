import { composeBackendEventSources, defineBackendEventSources } from "@goddard-ai/backend-plugin"
import { remoteRepoBackendEvents } from "@goddard-ai/remote-repo/backend"
import { describe, expect, test } from "bun:test"

import {
  canGitHubPrincipalAccessRepository,
  githubBackendEventSources,
  githubBackendRoutes,
  normalizeGitHubWebhookRequest,
} from "../src/backend.ts"

describe("github feature package", () => {
  test("exports selected feature entrypoints", () => {
    expect(githubBackendRoutes.webhooks.path.source).toBe("/webhooks")
    expect(githubBackendRoutes.webhooks.children.github.path?.source).toBe("/github")
    expect(Object.keys(githubBackendEventSources)).toEqual(["github"])
    expect(githubBackendEventSources.github.produces).toEqual(["remote_repo.event.received"])
  })

  test("registers as a source for declared remote repo events only", () => {
    expect(
      Object.keys(composeBackendEventSources([githubBackendEventSources], remoteRepoBackendEvents)),
    ).toEqual(["github"])

    expect(() =>
      composeBackendEventSources(
        [
          defineBackendEventSources({
            github: {
              produces: ["pull_request.feedback.received"],
              authorize: () => true,
            },
          }),
        ],
        remoteRepoBackendEvents,
      ),
    ).toThrow("Backend event source github produces unknown event: pull_request.feedback.received")
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
      name: "remote_repo.event.received",
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

  test("backend event source enforces repo authorization", async () => {
    const event = normalizeGitHubWebhookRequest({
      deliveryId: "delivery-1",
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
    })
    if (!event) {
      throw new Error("expected GitHub webhook to normalize")
    }
    const source = githubBackendEventSources.github

    await expect(
      Promise.resolve(
        source.authorize({
          principal: {
            kind: "github_user",
            githubUserId: 42,
            githubLogin: "alec",
            repositories: [{ owner: "goddard-ai", repo: "core" }],
          },
          event,
        }),
      ),
    ).resolves.toBe(true)

    await expect(
      Promise.resolve(
        source.authorize({
          principal: {
            kind: "github_user",
            githubUserId: 43,
            githubLogin: "bob",
            repositories: [{ owner: "goddard-ai", repo: "other" }],
          },
          event,
        }),
      ),
    ).resolves.toBe(false)
  })
})
