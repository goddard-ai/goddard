import { REMOTE_REPO_PULL_REQUEST_COMMENT_CREATED } from "@goddard-ai/remote-repo/backend"
import { expect, test } from "bun:test"

import { UserStream } from "../src/worker.ts"

test("user stream durable object fans out published events as ndjson", async () => {
  const stream = new UserStream()
  const controller = new AbortController()
  const response = await stream.fetch(
    new Request("https://user-stream.internal/subscribe", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ names: ["comment"] }),
      signal: controller.signal,
    }),
  )

  const eventPromise = readFirstNdjsonEvent(response)

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
  const payload = (await eventPromise) as { type: string; prNumber: number }
  expect(payload.type).toBe("comment")
  expect(payload.prNumber).toBe(1)

  controller.abort()
})

async function readFirstNdjsonEvent(response: Response): Promise<unknown> {
  if (!response.body) {
    throw new Error("Missing NDJSON response body")
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

    let separatorIndex = buffer.indexOf("\n")
    while (separatorIndex !== -1) {
      const line = buffer.slice(0, separatorIndex).trim()
      buffer = buffer.slice(separatorIndex + 1)

      if (line) {
        await reader.cancel()
        return JSON.parse(line)
      }

      separatorIndex = buffer.indexOf("\n")
    }
  }

  throw new Error("NDJSON stream ended before emitting data")
}
