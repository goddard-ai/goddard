import { InMemoryTokenStorage } from "@goddard-ai/storage"
import { afterEach, expect, test } from "vitest"
import { GoddardSdk } from "../src/sdk.ts"

const originalWebSocket = globalThis.WebSocket

afterEach(() => {
  if (originalWebSocket) {
    globalThis.WebSocket = originalWebSocket
  } else {
    Reflect.deleteProperty(globalThis, "WebSocket")
  }
  MockWebSocket.instances.length = 0
})

test("device flow stores token and whoami uses auth header", async () => {
  const storage = new InMemoryTokenStorage()

  const fetchImpl: typeof fetch = async (input, init) => {
    const url = String(input)
    if (url.endsWith("/auth/device/start")) {
      return jsonResponse(200, {
        deviceCode: "dev_1",
        userCode: "ABCD-1234",
        verificationUri: "https://github.com/login/device",
        expiresIn: 900,
        interval: 5,
      })
    }

    if (url.endsWith("/auth/device/complete")) {
      return jsonResponse(200, {
        token: "tok_1",
        githubUsername: "alec",
        githubUserId: 42,
      })
    }

    if (url.endsWith("/auth/session")) {
      expect(init?.headers && (init.headers as Record<string, string>).authorization).toBe(
        "Bearer tok_1",
      )
      return jsonResponse(200, {
        token: "tok_1",
        githubUsername: "alec",
        githubUserId: 42,
      })
    }

    return jsonResponse(404, { error: "not found" })
  }

  const sdk = new GoddardSdk({
    backendUrl: "http://127.0.0.1:8787",
    tokenStorage: storage,
    fetch: fetchImpl,
  })

  const start = await sdk.auth.startDeviceFlow()
  expect(start.deviceCode).toBe("dev_1")

  const session = await sdk.auth.completeDeviceFlow({
    deviceCode: start.deviceCode,
    githubUsername: "alec",
  })
  expect(session.githubUsername).toBe("alec")
  expect(await storage.getToken()).toBe("tok_1")

  const me = await sdk.auth.whoami()
  expect(me.githubUserId).toBe(42)
})

test("pr create requires authentication", async () => {
  const storage = new InMemoryTokenStorage()

  const fetchImpl: typeof fetch = async (input) => {
    const url = String(input)
    if (url.endsWith("/pr/create")) {
      return jsonResponse(200, {
        id: 1,
        number: 1,
        owner: "org",
        repo: "repo",
        title: "demo",
        body: "body",
        head: "feat",
        base: "main",
        url: "https://github.com/org/repo/pull/1",
        createdBy: "alec",
        createdAt: new Date().toISOString(),
      })
    }

    return jsonResponse(404, { error: "not found" })
  }

  const sdk = new GoddardSdk({
    backendUrl: "http://127.0.0.1:8787",
    tokenStorage: storage,
    fetch: fetchImpl,
  })

  await expect(() =>
    sdk.pr.create({ owner: "org", repo: "repo", title: "demo", head: "feat", base: "main" }),
  ).rejects.toThrow()

  await storage.setToken("tok_2")

  const pr = await sdk.pr.create({
    owner: "org",
    repo: "repo",
    title: "demo",
    head: "feat",
    base: "main",
  })
  expect(pr.number).toBe(1)
})

test("pr.isManaged returns managed status", async () => {
  const storage = new InMemoryTokenStorage()
  await storage.setToken("tok_pr")

  const fetchImpl: typeof fetch = async (input, init) => {
    const url = String(input)
    if (url.includes("/pr/managed?")) {
      expect(init?.headers && (init.headers as Record<string, string>).authorization).toBe(
        "Bearer tok_pr",
      )
      return jsonResponse(200, { managed: true })
    }
    return jsonResponse(404, { error: "not found" })
  }

  const sdk = new GoddardSdk({
    backendUrl: "http://127.0.0.1:8787",
    tokenStorage: storage,
    fetch: fetchImpl,
  })

  const managed = await sdk.pr.isManaged({ owner: "org", repo: "repo", prNumber: 12 })
  expect(managed).toBe(true)
})

test("stream emits error event for malformed payloads", async () => {
  const storage = new InMemoryTokenStorage()
  await storage.setToken("tok_stream")
  globalThis.WebSocket = MockWebSocket as any

  const sdk = new GoddardSdk({
    backendUrl: "http://127.0.0.1:8787",
    tokenStorage: storage,
    fetch: async () => jsonResponse(404, { error: "not found" }),
  })

  const sub = await sdk.stream.subscribe()
  const socket = MockWebSocket.instances[0]
  if (!socket) {
    throw new Error("Expected one WebSocket instance")
  }

  let errorMessage = ""
  sub.on("error", (error) => {
    errorMessage = error instanceof Error ? error.message : String(error)
  })

  socket.emitRawMessage("{")
  await new Promise((resolve) => setTimeout(resolve, 0))

  expect(errorMessage).toMatch(/Invalid stream payload/)
  sub.close()
})

test("stream.history uses the authenticated backend client route", async () => {
  const storage = new InMemoryTokenStorage()
  await storage.setToken("tok_history")

  const fetchImpl: typeof fetch = async (input, init) => {
    const url = String(input)
    if (url.includes("/stream/history?after=7")) {
      expect(init?.headers && (init.headers as Record<string, string>).authorization).toBe(
        "Bearer tok_history",
      )
      return jsonResponse(200, {
        events: [
          {
            id: 8,
            createdAt: new Date().toISOString(),
            event: {
              type: "comment",
              owner: "org",
              repo: "repo",
              prNumber: 4,
              author: "teammate",
              body: "ship it",
              reactionAdded: "eyes",
              createdAt: new Date().toISOString(),
            },
          },
        ],
      })
    }

    return jsonResponse(404, { error: "not found" })
  }

  const sdk = new GoddardSdk({
    backendUrl: "http://127.0.0.1:8787",
    tokenStorage: storage,
    fetch: fetchImpl,
  })

  await expect(sdk.stream.history({ after: 7 })).resolves.toEqual([
    expect.objectContaining({
      id: 8,
      event: expect.objectContaining({ type: "comment", prNumber: 4 }),
    }),
  ])
})

function jsonResponse(status: number, payload: unknown): Response {
  return new Response(JSON.stringify(payload), {
    status,
    headers: {
      "content-type": "application/json",
    },
  })
}

/** Minimal WebSocket mock for SDK stream tests. */
class MockWebSocket {
  static readonly CONNECTING = 0
  static readonly OPEN = 1
  static readonly CLOSING = 2
  static readonly CLOSED = 3
  static readonly instances: MockWebSocket[] = []

  readyState = MockWebSocket.CONNECTING
  onopen: ((event: Event) => void) | null = null
  onmessage: ((event: MessageEvent) => void) | null = null
  onerror: ((event: Event) => void) | null = null
  onclose: ((event: CloseEvent) => void) | null = null

  constructor(_url: string) {
    MockWebSocket.instances.push(this)
    queueMicrotask(() => {
      this.readyState = MockWebSocket.OPEN
      this.onopen?.({ type: "open" } as Event)
    })
  }

  send(_message: string): void {
    // No-op for tests.
  }

  close(code = 1000, reason = ""): void {
    this.readyState = MockWebSocket.CLOSED
    this.onclose?.({
      type: "close",
      code,
      reason,
      wasClean: true,
    } as CloseEvent)
  }

  emitRawMessage(data: string): void {
    this.onmessage?.({
      type: "message",
      data,
    } as MessageEvent)
  }
}
