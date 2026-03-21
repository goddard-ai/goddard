import { InMemoryBackendControlPlane, startBackendServer } from "@goddard-ai/backend"
import { InMemoryTokenStorage } from "@goddard-ai/storage"
import { afterEach, expect, test, vi } from "vitest"
import { createBackendClient } from "../src/index.ts"

const originalWebSocket = globalThis.WebSocket

afterEach(() => {
  vi.useRealTimers()
  if (originalWebSocket) {
    globalThis.WebSocket = originalWebSocket
  } else {
    Reflect.deleteProperty(globalThis, "WebSocket")
  }
  MockWebSocket.instances.length = 0
})

test("backend client creates PRs and checks managed status through rouzer route helpers", async () => {
  const controlPlane = new InMemoryBackendControlPlane()
  const server = await startBackendServer(controlPlane, { port: 0 })
  const baseUrl = `http://127.0.0.1:${server.port}`
  const tokenStorage = new InMemoryTokenStorage()

  try {
    const flow = controlPlane.startDeviceFlow({ githubUsername: "alec" })
    const session = controlPlane.completeDeviceFlow({
      deviceCode: flow.deviceCode,
      githubUsername: "alec",
    })
    await tokenStorage.setToken(session.token)

    const client = createBackendClient({ baseUrl, tokenStorage })
    const pr = await client.pr.create({
      owner: "goddard-ai",
      repo: "sdk",
      title: "Add backend client",
      body: "Ship it",
      head: "feat/backend-client",
      base: "main",
    })

    expect(pr.number).toBe(1)
    await expect(
      client.pr.isManaged({ owner: "goddard-ai", repo: "sdk", prNumber: pr.number }),
    ).resolves.toBe(true)
  } finally {
    await server.close()
  }
})

test("backend client manages auth session state through token storage", async () => {
  const controlPlane = new InMemoryBackendControlPlane()
  const server = await startBackendServer(controlPlane, { port: 0 })
  const baseUrl = `http://127.0.0.1:${server.port}`
  const tokenStorage = new InMemoryTokenStorage()

  try {
    const client = createBackendClient({ baseUrl, tokenStorage })
    const start = await client.auth.startDeviceFlow({ githubUsername: "alec" })
    const session = await client.auth.completeDeviceFlow({
      deviceCode: start.deviceCode,
      githubUsername: "alec",
    })

    await expect(tokenStorage.getToken()).resolves.toBe(session.token)
    await expect(client.auth.whoami()).resolves.toEqual(session)

    await client.auth.logout()
    await expect(tokenStorage.getToken()).resolves.toBeNull()
  } finally {
    await server.close()
  }
})

test("backend client subscribes over websocket and parses live events", async () => {
  const tokenStorage = new InMemoryTokenStorage()
  await tokenStorage.setToken("tok_stream")
  globalThis.WebSocket = MockWebSocket as any

  const client = createBackendClient({
    baseUrl: "http://127.0.0.1:8787",
    tokenStorage,
  })
  const subscription = await client.stream.subscribe()
  const socket = getOnlySocket()
  const eventPromise = new Promise<unknown>((resolve) => {
    subscription.on("event", resolve)
  })

  expect(socket.url).toBe("ws://127.0.0.1:8787/stream?token=tok_stream")

  socket.emitMessage({
    id: 1,
    event: {
      type: "pr.created",
      owner: "goddard-ai",
      repo: "sdk",
      prNumber: 1,
      title: "Stream me",
      author: "alec",
      createdAt: new Date().toISOString(),
    },
  })

  const event = (await eventPromise) as { type: string; prNumber: number }
  expect(event.type).toBe("pr.created")
  expect(event.prNumber).toBe(1)
  subscription.close()
})

test("backend client exposes history, emits message ids, and sends heartbeat pings", async () => {
  vi.useFakeTimers()
  const tokenStorage = new InMemoryTokenStorage()
  await tokenStorage.setToken("tok_history")
  globalThis.WebSocket = MockWebSocket as any

  const fetchImpl: typeof fetch = async (input, init) => {
    const url = String(input)
    if (url.includes("/stream/history")) {
      expect(init?.headers && (init.headers as Record<string, string>).authorization).toBe(
        "Bearer tok_history",
      )
      return new Response(
        JSON.stringify({
          events: [
            {
              id: 2,
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
          ],
        }),
        {
          status: 200,
          headers: {
            "content-type": "application/json",
          },
        },
      )
    }

    return new Response("not found", { status: 404 })
  }

  const client = createBackendClient({
    baseUrl: "http://127.0.0.1:8787",
    tokenStorage,
    fetchImpl,
  })
  const subscription = await client.stream.subscribe()
  const socket = getOnlySocket()
  const messagePromise = new Promise<unknown>((resolve) => {
    subscription.on("message", resolve)
  })

  socket.emitMessage({
    id: 1,
    event: {
      type: "pr.created",
      owner: "goddard-ai",
      repo: "sdk",
      prNumber: 1,
      title: "Stream me",
      author: "alec",
      createdAt: new Date().toISOString(),
    },
  })

  const message = (await messagePromise) as { id: number; event: { type: string } }
  expect(message.id).toBe(1)
  expect(message.event.type).toBe("pr.created")

  vi.advanceTimersByTime(30_000)
  expect(socket.sentMessages).toContain("ping")
  await expect(client.stream.history()).resolves.toEqual([
    expect.objectContaining({ id: 2, event: expect.objectContaining({ type: "comment" }) }),
  ])

  subscription.close()
})

test("backend client reconnects after unexpected websocket closes", async () => {
  vi.useFakeTimers()
  const tokenStorage = new InMemoryTokenStorage()
  await tokenStorage.setToken("tok_reconnect")
  globalThis.WebSocket = MockWebSocket as any

  const client = createBackendClient({
    baseUrl: "http://127.0.0.1:8787",
    tokenStorage,
    fetchImpl: async () => new Response("not found", { status: 404 }),
  })
  const subscription = await client.stream.subscribe()
  const firstSocket = getOnlySocket()

  firstSocket.emitClose(1012, "restart")
  vi.advanceTimersByTime(250)
  await Promise.resolve()

  expect(MockWebSocket.instances).toHaveLength(2)
  const secondSocket = MockWebSocket.instances[1]!
  const eventPromise = new Promise<unknown>((resolve) => {
    subscription.on("message", resolve)
  })

  secondSocket.emitMessage({
    id: 3,
    event: {
      type: "comment",
      owner: "goddard-ai",
      repo: "sdk",
      prNumber: 1,
      author: "teammate",
      body: "ship it",
      reactionAdded: "eyes",
      createdAt: new Date().toISOString(),
    },
  })

  await expect(eventPromise).resolves.toEqual(
    expect.objectContaining({ id: 3, event: expect.objectContaining({ type: "comment" }) }),
  )
  subscription.close()
})

function getOnlySocket(): MockWebSocket {
  const socket = MockWebSocket.instances[0]
  if (!socket) {
    throw new Error("Expected one mock WebSocket instance")
  }

  return socket
}

/** Minimal browser-style WebSocket mock used by the backend client tests. */
class MockWebSocket {
  static readonly CONNECTING = 0
  static readonly OPEN = 1
  static readonly CLOSING = 2
  static readonly CLOSED = 3
  static readonly instances: MockWebSocket[] = []

  readonly url: string
  readyState = MockWebSocket.CONNECTING
  sentMessages: string[] = []
  onopen: ((event: Event) => void) | null = null
  onmessage: ((event: MessageEvent) => void) | null = null
  onerror: ((event: Event) => void) | null = null
  onclose: ((event: CloseEvent) => void) | null = null

  constructor(url: string) {
    this.url = url
    MockWebSocket.instances.push(this)
    queueMicrotask(() => {
      this.readyState = MockWebSocket.OPEN
      this.onopen?.({ type: "open" } as Event)
    })
  }

  send(message: string): void {
    this.sentMessages.push(message)
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

  emitMessage(payload: unknown): void {
    this.onmessage?.({
      type: "message",
      data: JSON.stringify(payload),
    } as MessageEvent)
  }

  emitClose(code: number, reason: string): void {
    this.readyState = MockWebSocket.CLOSED
    this.onclose?.({
      type: "close",
      code,
      reason,
      wasClean: false,
    } as CloseEvent)
  }
}
