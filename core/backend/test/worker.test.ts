import { test, assert } from "vitest"
import { UserStream } from "../src/worker.ts"

test("user stream durable object fans out published events to subscribers", async () => {
  const stream = new UserStream()
  const controller = new AbortController()
  const response = await stream.fetch(
    new Request("https://user-stream.internal/subscribe", {
      signal: controller.signal,
    }),
  )

  const eventPromise = readFirstNdjsonEvent(response)

  const publishResponse = await stream.fetch(
    new Request("https://user-stream.internal/publish", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({
        record: {
          id: 9,
          createdAt: new Date().toISOString(),
          event: {
            type: "comment",
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

  assert.equal(publishResponse.status, 204)
  const payload = (await eventPromise) as { id?: number; event: { type: string; prNumber: number } }
  assert.equal(payload.id, 9)
  assert.equal(payload.event.type, "comment")
  assert.equal(payload.event.prNumber, 1)

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
      const rawEvent = buffer.slice(0, separatorIndex).trim()
      buffer = buffer.slice(separatorIndex + 1)

      if (rawEvent) {
        await reader.cancel()
        return JSON.parse(rawEvent)
      }

      separatorIndex = buffer.indexOf("\n")
    }
  }

  throw new Error("NDJSON stream ended before emitting data")
}
