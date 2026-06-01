import { expect, test } from "bun:test"

import { normalizeGitHubWebhookEvent, remoteRepoBackendRoutes } from "../src/backend.ts"

test("backend routes keep their logical remote-repo resource grouping", () => {
  expect(remoteRepoBackendRoutes.remoteRepo.path.source).toBe("/remote-repo")
  expect(remoteRepoBackendRoutes.remoteRepo.children.stream.path?.source).toBe("/stream")
  expect(remoteRepoBackendRoutes.webhooks.path.source).toBe("/webhooks")
  expect(remoteRepoBackendRoutes.webhooks.children.github.path?.source).toBe("/github")
})

test("normalizes GitHub comment webhooks into remote repository events", () => {
  expect(
    normalizeGitHubWebhookEvent(
      {
        type: "issue_comment",
        owner: "goddard-ai",
        repo: "sdk",
        prNumber: 12,
        author: "alec",
        body: "looks good",
      },
      "2026-01-01T00:00:00.000Z",
    ),
  ).toEqual({
    type: "comment",
    owner: "goddard-ai",
    repo: "sdk",
    prNumber: 12,
    author: "alec",
    body: "looks good",
    reactionAdded: "eyes",
    createdAt: "2026-01-01T00:00:00.000Z",
  })
})
