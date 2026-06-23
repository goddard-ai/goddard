import { REMOTE_REPO_PULL_REQUEST_COMMENT_CREATED } from "@goddard-ai/remote-repo/backend"
import { expect, test } from "bun:test"

import { UserStream } from "../src/worker.ts"

test("user stream durable object fans out published events to subscribers", async () => {
  const stream = new UserStream()
  const controller = new AbortController()
  const response = await stream.fetch(
    new Request("https://user-stream.internal/subscribe", {
      signal: controller.signal,
    }),
  )

  const eventPromise = readFirstSseEvent(response)

  const publishResponse = await stream.fetch(
    new Request("https://user-stream.internal/publish", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({
        event: {
          name: REMOTE_REPO_PULL_REQUEST_COMMENT_CREATED,
          payload: {
            type: "comment",
            provider: "github",
            owner: "goddard-ai",
            repo: "sdk",
            prNumber: 1,
            author: "teammate",
            body: "looks good",
            reactionAdded: "eyes",
            createdAt: new Date().toISOString(),
          },
        },
      }),
    }),
  )

  expect(publishResponse.status).toBe(204)
  const payload = (await eventPromise) as {
    name: string
    payload: { type: string; prNumber: number }
  }
  expect(payload.name).toBe(REMOTE_REPO_PULL_REQUEST_COMMENT_CREATED)
  expect(payload.payload.type).toBe("comment")
  expect(payload.payload.prNumber).toBe(1)

  controller.abort()
})

test("user stream durable object applies envelope filters", async () => {
  const stream = new UserStream()
  const matchingController = new AbortController()
  const ignoredController = new AbortController()
  const filter = encodeURIComponent(
    JSON.stringify({
      names: [REMOTE_REPO_PULL_REQUEST_COMMENT_CREATED],
      where: [{ path: "repo", equals: "sdk" }],
    }),
  )
  const ignoredFilter = encodeURIComponent(
    JSON.stringify({
      names: [REMOTE_REPO_PULL_REQUEST_COMMENT_CREATED],
      where: [{ path: "repo", equals: "other" }],
    }),
  )
  const matchingResponse = await stream.fetch(
    new Request(`https://user-stream.internal/subscribe?filter=${filter}`, {
      signal: matchingController.signal,
    }),
  )
  const ignoredResponse = await stream.fetch(
    new Request(`https://user-stream.internal/subscribe?filter=${ignoredFilter}`, {
      signal: ignoredController.signal,
    }),
  )

  const matchingEvent = readFirstSseEvent(matchingResponse)
  const ignoredEvent = readFirstSseEvent(ignoredResponse)

  await stream.fetch(
    new Request("https://user-stream.internal/publish", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({
        event: {
          name: REMOTE_REPO_PULL_REQUEST_COMMENT_CREATED,
          payload: {
            type: "comment",
            provider: "github",
            owner: "goddard-ai",
            repo: "sdk",
            prNumber: 1,
            author: "teammate",
            body: "looks good",
            reactionAdded: "eyes",
            createdAt: new Date().toISOString(),
          },
        },
      }),
    }),
  )

  const payload = (await matchingEvent) as { payload: { repo: string } }
  expect(payload.payload.repo).toBe("sdk")
  await expectNoEvent(ignoredEvent, 25)

  matchingController.abort()
  ignoredController.abort()
})

async function readFirstSseEvent(response: Response): Promise<unknown> {
  if (!response.body) {
    throw new Error("Missing SSE response body")
  }

  const reader = response.body.getReader()
  const decoder = new TextDecoder()
  let buffer = ""

  while (true) {
    const { value, done } = await reader.read()
    if (done) {
      break
    }

    buffer += decoder.decode(value, { stream: true })

    let separatorIndex = buffer.indexOf("\n\n")
    while (separatorIndex !== -1) {
      const rawEvent = buffer.slice(0, separatorIndex)
      buffer = buffer.slice(separatorIndex + 2)

      const dataLines = rawEvent
        .split("\n")
        .filter((line) => line.startsWith("data:"))
        .map((line) => line.slice("data:".length).trimStart())

      if (dataLines.length > 0) {
        await reader.cancel()
        return JSON.parse(dataLines.join("\n"))
      }

      separatorIndex = buffer.indexOf("\n\n")
    }
  }

  throw new Error("SSE stream ended before emitting data")
}

async function expectNoEvent(promise: Promise<unknown>, timeoutMs: number) {
  const result = await Promise.race([
    promise.then(() => "event"),
    new Promise<"timeout">((resolve) => setTimeout(() => resolve("timeout"), timeoutMs)),
  ])
  expect(result).toBe("timeout")
}
