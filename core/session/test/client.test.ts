import { describe, expect, test, vi } from "vitest"

const {
  clientConnectionArgs,
  mockReconnectSockets,
  sessionStorageGet,
  wsConstructor,
} = vi.hoisted(() => ({
  clientConnectionArgs: [] as any[],
  mockReconnectSockets: [] as any[],
  sessionStorageGet: vi.fn(async () => ({ serverAddress: "http://localhost:3001" })),
  wsConstructor: class MockNodeWebSocket {},
}))

vi.mock("@goddard-ai/storage", () => ({
  SessionStorage: {
    get: sessionStorageGet,
  },
}))

vi.mock("ws", () => ({
  default: wsConstructor,
}))

vi.mock("reconnecting-websocket", () => ({
  default: class MockReconnectingWebSocket {
    url: string
    options: Record<string, unknown>
    listeners = new Map<string, Set<(event: any) => void>>()
    close = vi.fn()
    send = vi.fn()

    constructor(url: string, _protocols: string[], options: Record<string, unknown>) {
      this.url = url
      this.options = options
      mockReconnectSockets.push(this)
      queueMicrotask(() => this.dispatch("open", { type: "open" }))
    }

    addEventListener(type: string, listener: (event: any) => void) {
      const listeners = this.listeners.get(type) ?? new Set()
      listeners.add(listener)
      this.listeners.set(type, listeners)
    }

    removeEventListener(type: string, listener: (event: any) => void) {
      this.listeners.get(type)?.delete(listener)
    }

    dispatch(type: string, event: any) {
      this.listeners.get(type)?.forEach((listener) => listener(event))
    }
  },
}))

vi.mock("@agentclientprotocol/sdk", () => ({
  ndJsonStream: vi.fn(() => ({
    readable: new ReadableStream(),
    writable: new WritableStream(),
  })),
  ClientSideConnection: class MockClientSideConnection {
    prompt = vi.fn()
    cancel = vi.fn()

    constructor(...args: any[]) {
      clientConnectionArgs.push(args)
    }
  },
}))

describe("runAgent", () => {
  test("uses reconnecting-websocket with the node ws constructor", async () => {
    const { runAgent } = await import("../src/client.js")

    await runAgent({ sessionId: "session-1", agentName: "demo-agent" } as any, {} as any)

    expect(sessionStorageGet).toHaveBeenCalledWith("session-1")
    expect(mockReconnectSockets).toHaveLength(1)
    expect(mockReconnectSockets[0].url).toBe("ws://localhost:3001/acp")
    expect(mockReconnectSockets[0].options.WebSocket).toBe(wsConstructor)
    expect(clientConnectionArgs).toHaveLength(1)
  })
})
