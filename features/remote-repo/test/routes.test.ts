import { expect, test } from "bun:test"

import {
  defineRemoteRepoEventHandler,
  dispatchRemoteRepoEvent,
  normalizeGitHubWebhookEvent,
  remoteRepoBackendEvents,
  remoteRepoBackendRoutes,
} from "../src/backend.ts"

test("backend routes keep their logical remote-repo resource grouping", () => {
  expect(remoteRepoBackendRoutes.remoteRepo.path.source).toBe("/remote-repo")
  expect(remoteRepoBackendRoutes.remoteRepo.children.stream.path?.source).toBe("/stream")
  expect("webhooks" in remoteRepoBackendRoutes).toBe(false)
})

test("owns the remote repository backend event contract", () => {
  const event = {
    type: "comment",
    owner: "goddard-ai",
    repo: "sdk",
    prNumber: 12,
    author: "alec",
    body: "looks good",
    reactionAdded: "eyes",
    createdAt: "2026-01-01T00:00:00.000Z",
  }

  expect(Object.keys(remoteRepoBackendEvents)).toEqual(["remote_repo.event.received"])
  expect(
    remoteRepoBackendEvents["remote_repo.event.received"].payload.safeParse(event).success,
  ).toBe(true)
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

test("dispatches remote repository events to matching feature handlers", async () => {
  const handled: string[] = []
  const event = normalizeGitHubWebhookEvent(
    {
      type: "issue_comment",
      owner: "goddard-ai",
      repo: "sdk",
      prNumber: 12,
      author: "alec",
      body: "looks good",
    },
    "2026-01-01T00:00:00.000Z",
  )

  await dispatchRemoteRepoEvent(event, [
    defineRemoteRepoEventHandler({
      name: "ignored",
      canHandle: () => false,
      handle: () => handled.push("ignored"),
    }),
    defineRemoteRepoEventHandler({
      name: "pull-request",
      canHandle: (input) => input.type === "comment",
      handle: (input) => handled.push(input.type),
    }),
  ])

  expect(handled).toEqual(["comment"])
})
