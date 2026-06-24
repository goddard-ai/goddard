import type { DaemonLogService } from "@goddard-ai/daemon-plugin"
import { REMOTE_REPO_PULL_REQUEST_COMMENT_CREATED } from "@goddard-ai/remote-repo/backend"
import type { RepoPullRequestCommentCreatedEvent } from "@goddard-ai/remote-repo/schema"
import { expect, test } from "bun:test"

import {
  createPullRequestFeedbackHandler,
  isFeedbackBackendEvent,
  isFeedbackEvent,
} from "../src/daemon/feedback.ts"

test("pull request feedback handler ignores non-feedback backend events", () => {
  expect(isFeedbackEvent({ type: "pr.created", owner: "acme" })).toBe(false)
  expect(isFeedbackEvent(createFeedbackEvent())).toBe(true)
  expect(
    isFeedbackBackendEvent({
      name: REMOTE_REPO_PULL_REQUEST_COMMENT_CREATED,
      payload: createFeedbackEvent(),
    }),
  ).toBe(true)
})

test("pull request feedback handler emits ignored events for unmanaged pull requests", async () => {
  const emitted: Array<{ name: string; payload: unknown }> = []
  const handler = createPullRequestFeedbackHandler({
    backend: {
      pullRequests: {
        managed: async () => ({ managed: false }),
      },
    },
    db: createFeedbackStore("/tmp/repo"),
    events: createEventBus(emitted),
    log: createLogService([]),
    session: {
      newSession: async () => {
        throw new Error("session should not start")
      },
    },
  })

  await handler.handle(createFeedbackBackendEvent())

  expect(emitted).toEqual([
    {
      name: "pull_request.feedback.ignored",
      payload: {
        repository: "acme/widgets",
        owner: "acme",
        repo: "widgets",
        prNumber: 12,
        feedbackType: "comment",
        reason: "unmanaged_pr",
      },
    },
  ])
})

test("pull request feedback handler launches one-shot sessions for managed feedback", async () => {
  const emitted: Array<{ name: string; payload: unknown }> = []
  const requests: unknown[] = []
  const handler = createPullRequestFeedbackHandler({
    backend: {
      pullRequests: {
        managed: async () => ({ managed: true }),
      },
    },
    db: createFeedbackStore("/tmp/repo"),
    events: createEventBus(emitted),
    log: createLogService([]),
    session: {
      newSession: async (input) => {
        requests.push(input.request)
      },
    },
  })

  await handler.handle(createFeedbackBackendEvent())

  expect(requests).toHaveLength(1)
  expect(requests[0]).toMatchObject({
    cwd: "/tmp/repo",
    worktree: { enabled: true },
    oneShot: true,
    repository: "acme/widgets",
    prNumber: 12,
  })
  expect(emitted).toEqual([
    {
      name: "pull_request.feedback.launched",
      payload: {
        repository: "acme/widgets",
        owner: "acme",
        repo: "widgets",
        prNumber: 12,
        feedbackType: "comment",
      },
    },
    {
      name: "pull_request.feedback.finished",
      payload: {
        repository: "acme/widgets",
        owner: "acme",
        repo: "widgets",
        prNumber: 12,
        feedbackType: "comment",
        exitCode: 0,
      },
    },
  ])
})

test("pull request feedback handler emits failed and finished events when the repository lookup fails", async () => {
  const emitted: Array<{ name: string; payload: unknown }> = []
  const handler = createPullRequestFeedbackHandler({
    backend: {
      pullRequests: {
        managed: async () => ({ managed: true }),
      },
    },
    db: createFeedbackStore(null),
    events: createEventBus(emitted),
    log: createLogService([]),
    session: {
      newSession: async () => {
        throw new Error("session should not start")
      },
    },
  })

  await handler.handle(createFeedbackBackendEvent())

  expect(emitted).toEqual([
    {
      name: "pull_request.feedback.failed",
      payload: {
        repository: "acme/widgets",
        owner: "acme",
        repo: "widgets",
        prNumber: 12,
        feedbackType: "comment",
        phase: "repository_lookup",
        errorMessage: "Managed pull request repository is unavailable",
      },
    },
    {
      name: "pull_request.feedback.finished",
      payload: {
        repository: "acme/widgets",
        owner: "acme",
        repo: "widgets",
        prNumber: 12,
        feedbackType: "comment",
        exitCode: 1,
      },
    },
  ])
})

test("pull request feedback handler emits failed events when session creation throws", async () => {
  const emitted: Array<{ name: string; payload: unknown }> = []
  const handler = createPullRequestFeedbackHandler({
    backend: {
      pullRequests: {
        managed: async () => ({ managed: true }),
      },
    },
    db: createFeedbackStore("/tmp/repo"),
    events: createEventBus(emitted),
    log: createLogService([]),
    session: {
      newSession: async () => {
        throw new Error("launch failed")
      },
    },
  })

  await handler.handle(createFeedbackBackendEvent())

  expect(emitted).toEqual([
    {
      name: "pull_request.feedback.launched",
      payload: {
        repository: "acme/widgets",
        owner: "acme",
        repo: "widgets",
        prNumber: 12,
        feedbackType: "comment",
      },
    },
    {
      name: "pull_request.feedback.failed",
      payload: {
        repository: "acme/widgets",
        owner: "acme",
        repo: "widgets",
        prNumber: 12,
        feedbackType: "comment",
        phase: "session_create",
        errorMessage: "launch failed",
      },
    },
  ])
})

test("pull request feedback handler emits coalesced events for concurrent feedback", async () => {
  const emitted: Array<{ name: string; payload: unknown }> = []
  const sessionStarted = createDeferred<void>()
  const releaseSession = createDeferred<void>()
  const handler = createPullRequestFeedbackHandler({
    backend: {
      pullRequests: {
        managed: async () => ({ managed: true }),
      },
    },
    db: createFeedbackStore("/tmp/repo"),
    events: createEventBus(emitted),
    log: createLogService([]),
    session: {
      newSession: async () => {
        sessionStarted.resolve()
        await releaseSession.promise
      },
    },
  })

  const firstHandle = handler.handle(createFeedbackBackendEvent())
  await sessionStarted.promise
  await handler.handle(createFeedbackBackendEvent())

  expect(emitted).toEqual([
    {
      name: "pull_request.feedback.launched",
      payload: {
        repository: "acme/widgets",
        owner: "acme",
        repo: "widgets",
        prNumber: 12,
        feedbackType: "comment",
      },
    },
    {
      name: "pull_request.feedback.coalesced",
      payload: {
        repository: "acme/widgets",
        owner: "acme",
        repo: "widgets",
        prNumber: 12,
        feedbackType: "comment",
      },
    },
  ])

  releaseSession.resolve()
  await firstHandle

  expect(emitted).toEqual([
    {
      name: "pull_request.feedback.launched",
      payload: {
        repository: "acme/widgets",
        owner: "acme",
        repo: "widgets",
        prNumber: 12,
        feedbackType: "comment",
      },
    },
    {
      name: "pull_request.feedback.coalesced",
      payload: {
        repository: "acme/widgets",
        owner: "acme",
        repo: "widgets",
        prNumber: 12,
        feedbackType: "comment",
      },
    },
    {
      name: "pull_request.feedback.finished",
      payload: {
        repository: "acme/widgets",
        owner: "acme",
        repo: "widgets",
        prNumber: 12,
        feedbackType: "comment",
        exitCode: 0,
      },
    },
  ])
})

function createFeedbackEvent(): RepoPullRequestCommentCreatedEvent {
  return {
    type: "comment",
    provider: "github",
    owner: "acme",
    repo: "widgets",
    prNumber: 12,
    author: "alice",
    body: "Please update this.",
    reactionAdded: "eyes",
    createdAt: new Date().toISOString(),
  }
}

function createFeedbackBackendEvent() {
  return {
    name: REMOTE_REPO_PULL_REQUEST_COMMENT_CREATED,
    payload: createFeedbackEvent(),
  }
}

function createFeedbackStore(cwd: string | null) {
  return {
    pullRequests: {
      first: () =>
        cwd == null
          ? null
          : {
              id: "pr_1" as const,
              host: "github" as const,
              owner: "acme",
              repo: "widgets",
              prNumber: 12,
              cwd,
              updatedAt: Date.now(),
            },
    },
  }
}

function createEventBus(emitted: Array<{ name: string; payload: unknown }>) {
  return {
    emit: async (name: string, payload: unknown) => {
      emitted.push({ name, payload })
    },
  } as never
}

function createLogService(
  logs: Array<{ event: string; fields: Record<string, unknown> }>,
): DaemonLogService {
  return {
    createLogger: () => ({
      log: (event, fields = {}) => {
        logs.push({ event, fields })
      },
      snapshot: () => createLogService(logs).createLogger(),
    }),
    createDebug: () => () => {},
    isVerboseLogging: () => false,
    createPayloadPreview: (value) => value,
    createChunkPreview: (value) => ({
      text: new TextDecoder().decode(value),
      byteLength: value.byteLength,
      truncated: false,
    }),
  }
}

function createDeferred<T>() {
  let resolve: (value: T | PromiseLike<T>) => void = () => {}
  let reject: (reason?: unknown) => void = () => {}
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise
    reject = rejectPromise
  })
  return { promise, resolve, reject }
}
