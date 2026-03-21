import { expect, test } from "vitest"
import { UserStream } from "../src/worker.ts"

test("user stream durable object upgrades sockets, stores attachments, and fans out published events", async () => {
  const context = new FakeUserStreamContext()
  const runtime = new FakeUserStreamRuntime()
  const stream = new UserStream(context as any, {} as any, runtime as any)

  const upgradeResponse = await stream.fetch(
    new Request("https://user-stream.internal/stream", {
      headers: {
        upgrade: "websocket",
        "x-github-username": "alec",
      },
    }),
  )

  expect(upgradeResponse.status).toBe(101)
  expect(context.autoResponsePair).toEqual({ request: "ping", response: "pong" })
  expect(context.getWebSockets()).toHaveLength(1)
  expect(context.getWebSockets()[0]?.deserializeAttachment()).toEqual({ githubUsername: "alec" })

  const otherSocket = new FakeSocket()
  otherSocket.serializeAttachment({ githubUsername: "bob" })
  context.acceptWebSocket(otherSocket as any)

  const publishResponse = await stream.fetch(
    new Request("https://user-stream.internal/publish", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({
        githubUsername: "alec",
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

  expect(publishResponse.status).toBe(204)
  expect(context.getWebSockets()[0]?.sentMessages).toHaveLength(1)
  expect(JSON.parse(context.getWebSockets()[0]?.sentMessages[0] ?? "{}")).toMatchObject({
    id: 9,
    event: {
      type: "comment",
      prNumber: 1,
    },
  })
  expect(otherSocket.sentMessages).toHaveLength(0)
})

/** Minimal fake Durable Object hibernation context used by the worker tests. */
class FakeUserStreamContext {
  autoResponsePair: { request: string; response: string } | undefined
  #sockets: FakeSocket[] = []

  acceptWebSocket(socket: FakeSocket): void {
    this.#sockets.push(socket)
  }

  getWebSockets(): FakeSocket[] {
    return [...this.#sockets]
  }

  setWebSocketAutoResponse(pair: { request: string; response: string }): void {
    this.autoResponsePair = pair
  }
}

/** Test runtime that replaces Cloudflare WebSocket primitives with local fakes. */
class FakeUserStreamRuntime {
  createWebSocketPair(): { client: FakeSocket; server: FakeSocket } {
    return {
      client: new FakeSocket(),
      server: new FakeSocket(),
    }
  }

  createAutoResponsePair(
    request: string,
    response: string,
  ): {
    request: string
    response: string
  } {
    return { request, response }
  }

  createUpgradeResponse(): Response {
    return { status: 101 } as Response
  }
}

/** Fake socket implementation that records frames and persisted attachments. */
class FakeSocket {
  sentMessages: string[] = []
  #attachment: unknown

  send(message: string): void {
    this.sentMessages.push(message)
  }

  close(): void {
    // No-op for tests.
  }

  serializeAttachment(attachment: unknown): void {
    this.#attachment = attachment
  }

  deserializeAttachment(): unknown {
    return this.#attachment
  }
}
