import { expect, test } from "bun:test"

import {
  createRemoteRepoBackendEvent,
  defineRemoteRepoEventHandler,
  dispatchRemoteRepoEvent,
  REMOTE_REPO_PULL_REQUEST_COMMENT_CREATED,
  REMOTE_REPO_PULL_REQUEST_CREATED,
  REMOTE_REPO_PULL_REQUEST_REVIEW_SUBMITTED,
  remoteRepoBackendEvents,
  remoteRepoBackendEventSources,
  remoteRepoBackendPlugin,
  remoteRepoBackendRoutes,
} from "../src/backend.ts"

test("backend routes keep their logical remote-repo resource grouping", () => {
  expect(remoteRepoBackendRoutes.remoteRepo.path.source).toBe("/remote-repo")
  expect(remoteRepoBackendRoutes.remoteRepo.children.stream.path?.source).toBe("/stream")
  expect("webhooks" in remoteRepoBackendRoutes).toBe(false)
  expect(remoteRepoBackendPlugin.name).toBe("remote-repo")
  expect(remoteRepoBackendPlugin.routes?.remoteRepo.path.source).toBe("/remote-repo")
  expect(remoteRepoBackendPlugin.events).toBe(remoteRepoBackendEvents)
  expect(remoteRepoBackendPlugin.eventSources).toBe(remoteRepoBackendEventSources)
})

test("owns the remote repository backend event contract", () => {
  const event = {
    type: "comment",
    provider: "example",
    owner: "goddard-ai",
    repo: "sdk",
    prNumber: 12,
    author: "alec",
    body: "looks good",
    reactionAdded: "eyes",
    createdAt: "2026-01-01T00:00:00.000Z",
  }

  expect(Object.keys(remoteRepoBackendEvents)).toEqual([
    REMOTE_REPO_PULL_REQUEST_COMMENT_CREATED,
    REMOTE_REPO_PULL_REQUEST_REVIEW_SUBMITTED,
    REMOTE_REPO_PULL_REQUEST_CREATED,
  ])
  expect(
    remoteRepoBackendEvents[REMOTE_REPO_PULL_REQUEST_COMMENT_CREATED].payload.safeParse(event)
      .success,
  ).toBe(true)
  expect(
    remoteRepoBackendEvents[REMOTE_REPO_PULL_REQUEST_COMMENT_CREATED].payload.safeParse({
      ...event,
      provider: undefined,
    }).success,
  ).toBe(false)
  expect(createRemoteRepoBackendEvent(event)).toEqual({
    name: REMOTE_REPO_PULL_REQUEST_COMMENT_CREATED,
    payload: event,
  })
})

test("owns remote repository backend event source authorization", async () => {
  const source = remoteRepoBackendEventSources["remote-repo"]
  const event = createRemoteRepoBackendEvent({
    type: "comment",
    provider: "github",
    owner: "goddard-ai",
    repo: "sdk",
    prNumber: 12,
    author: "alec",
    body: "looks good",
    reactionAdded: "eyes",
    createdAt: "2026-01-01T00:00:00.000Z",
  })

  await expect(
    Promise.resolve(
      source.authorize({
        principal: {
          id: "github:2997745",
          providerIdentities: [
            {
              provider: "github",
              subject: "2997745",
              displayName: "alec",
            },
          ],
          repositories: [{ provider: "github", owner: "goddard-ai", repo: "sdk" }],
        },
        event,
      }),
    ),
  ).resolves.toBe(true)
  await expect(
    Promise.resolve(
      source.authorize({
        principal: {
          id: "github:2997745",
          providerIdentities: [
            {
              provider: "github",
              subject: "2997745",
              displayName: "alec",
            },
          ],
          repositories: [{ provider: "github", owner: "goddard-ai", repo: "other" }],
        },
        event,
      }),
    ),
  ).resolves.toBe(false)
})

test("creates provider-qualified remote repository events", () => {
  expect(
    createRemoteRepoBackendEvent({
      type: "comment",
      provider: "github",
      owner: "goddard-ai",
      repo: "sdk",
      prNumber: 12,
      author: "alec",
      body: "looks good",
      reactionAdded: "eyes",
      createdAt: "2026-01-01T00:00:00.000Z",
    }).payload,
  ).toEqual({
    type: "comment",
    provider: "github",
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
  const event = {
    type: "comment",
    provider: "github",
    owner: "goddard-ai",
    repo: "sdk",
    prNumber: 12,
    author: "alec",
    body: "looks good",
    reactionAdded: "eyes",
    createdAt: "2026-01-01T00:00:00.000Z",
  } as const

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
