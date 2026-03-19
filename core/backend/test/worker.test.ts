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

  const eventPromise = readFirstSseEvent(response)

  const publishResponse = await stream.fetch(
    new Request("https://user-stream.internal/publish", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({
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
      }),
    }),
  )

  assert.equal(publishResponse.status, 204)
  const payload = (await eventPromise) as { event: { type: string; prNumber: number } }
  assert.equal(payload.event.type, "comment")
  assert.equal(payload.event.prNumber, 1)

  controller.abort()
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
