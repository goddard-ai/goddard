import { describe, expect, test, vi } from "vitest"

const { wsInstances } = vi.hoisted(() => ({
  wsInstances: [] as any[],
}))

vi.mock("ws", () => {
  class MockWebSocket {
    private listeners = new Map<string, Set<(...args: any[]) => void>>()
    send = vi.fn()
    close = vi.fn(() => this.emit("close"))

    on(event: string, listener: (...args: any[]) => void) {
      const set = this.listeners.get(event) ?? new Set()
      set.add(listener)
      this.listeners.set(event, set)
    }

    emit(event: string, ...args: any[]) {
      this.listeners.get(event)?.forEach((listener) => listener(...args))
    }
  }

  class MockWebSocketServer {
    close = vi.fn()

    handleUpgrade(_req: unknown, _socket: unknown, _head: unknown, cb: (ws: any) => void) {
      const ws = new MockWebSocket()
      wsInstances.push(ws)
      cb(ws)
    }
  }

  return {
    default: MockWebSocket,
    WebSocketServer: MockWebSocketServer,
  }
})

describe("createWebSocketHandler", () => {
  test("broadcast can exclude the source client", async () => {
    const { createWebSocketHandler } = await import("../src/node/websocket-server.js")

    const upgradeListeners: Array<(req: any, socket: any, head: any) => void> = []
    const handler = createWebSocketHandler({
      async onMessage() {
        // no-op
      },
    })

    handler.listen(
      {
        url: "http://localhost:3000",
        node: {
          server: {
            on: (_event: string, listener: (req: any, socket: any, head: any) => void) => {
              upgradeListeners.push(listener)
            },
          },
        },
      } as any,
      "/acp",
    )

    upgradeListeners[0]({ url: "/acp" }, { destroy: vi.fn() }, Buffer.from(""))
    upgradeListeners[0]({ url: "/acp" }, { destroy: vi.fn() }, Buffer.from(""))

    const [clientA, clientB] = wsInstances
    handler.broadcast({ kind: "test" }, { exclude: clientA } as any)

    expect(clientA.send).not.toHaveBeenCalled()
    expect(clientB.send).toHaveBeenCalledOnce()
  })
})
