import { describe, expect, it, vi } from "vitest"
import WebSocket from "ws"

import type { NormalizedSessionPayload, SessionClientEvent } from "@goddard-ai/session-protocol"
import { startServer } from "../src/server"
import { SessionDriver } from "../src/drivers/types"

class MockDriver extends SessionDriver {
  readonly name = "pty" as const
  readonly start = vi.fn<(input: { resume?: string }) => void>()
  readonly sendEvent = vi.fn<(event: SessionClientEvent) => void>()
  readonly close = vi.fn<() => void>()

  getCapabilities() {
    return {
      terminal: {
        enabled: true,
        canResize: true,
        hasScreenState: true,
      },
      normalizedOutput: true,
    }
  }
}

function waitForOpen(ws: WebSocket): Promise<void> {
  return new Promise((resolve, reject) => {
    ws.once("open", () => resolve())
    ws.once("error", reject)
  })
}

function createMessageQueue(ws: WebSocket) {
  const queue: unknown[] = []
  const waiters: Array<(message: unknown) => void> = []

  ws.on("message", (data) => {
    const message = JSON.parse(data.toString()) as unknown
    const waiter = waiters.shift()
    if (waiter) {
      waiter(message)
      return
    }

    queue.push(message)
  })

  return {
    async next(): Promise<unknown> {
      if (queue.length > 0) {
        return queue.shift()
      }

      return await new Promise((resolve, reject) => {
        waiters.push(resolve)
        ws.once("error", reject)
      })
    },
  }
}

describe("server", () => {
  it("returns JSON-RPC parse errors for malformed payloads", async () => {
    const driver = new MockDriver()
    const server = await startServer({ transport: "tcp", driver })
    const ws = new WebSocket(server.endpoint.url)
    await waitForOpen(ws)
    const messages = createMessageQueue(ws)

    ws.send("{bad json")
    const message = (await messages.next()) as any

    expect(message.error.code).toBe(-32700)

    ws.close()
    await server.close()
  })

  it("returns JSON-RPC errors for invalid request params", async () => {
    const driver = new MockDriver()
    const server = await startServer({ transport: "tcp", driver })
    expect(driver.start).toHaveBeenCalledWith({})
    const ws = new WebSocket(server.endpoint.url)
    await waitForOpen(ws)
    const messages = createMessageQueue(ws)

    ws.send(
      JSON.stringify({
        jsonrpc: "2.0",
        id: 1,
        method: "session_initialize",
        params: {},
      }),
    )
    const initError = (await messages.next()) as any
    expect(initError.error.message).toContain("Invalid session_initialize params")

    ws.send(
      JSON.stringify({
        jsonrpc: "2.0",
        id: 2,
        method: "session_send_event",
        params: { event: { type: "input.text", text: "" } },
      }),
    )
    const eventError = (await messages.next()) as any
    expect(eventError.error.message).toContain("Invalid session_send_event params")
    expect(driver.sendEvent).not.toHaveBeenCalled()

    ws.close()
    await server.close()
  })

  it("supports initialize, event sending, state reads, and session_event notifications", async () => {
    const driver = new MockDriver()
    const server = await startServer({
      transport: "tcp",
      driver,
      startupInput: { resume: "resume-123" },
    })
    expect(driver.start).toHaveBeenCalledWith({ resume: "resume-123" })
    const ws = new WebSocket(server.endpoint.url)
    await waitForOpen(ws)
    const messages = createMessageQueue(ws)

    ws.send(JSON.stringify({ jsonrpc: "2.0", id: 1, method: "session_initialize" }))
    const infoResponse = (await messages.next()) as any
    expect(infoResponse.result.driver).toBe("pty")
    expect(infoResponse.result.protocolVersion).toBe(1)
    expect(infoResponse.result.capabilities.terminal.canResize).toBe(true)
    expect(infoResponse.result.capabilities.normalizedOutput).toBe(true)

    ws.send(
      JSON.stringify({
        jsonrpc: "2.0",
        id: 2,
        method: "session_send_event",
        params: { event: { type: "input.terminal", data: "a" } },
      }),
    )
    const sendResponse = (await messages.next()) as any
    expect(sendResponse.result.ok).toBe(true)
    expect(driver.sendEvent).toHaveBeenCalledWith({ type: "input.terminal", data: "a" })

    ws.send(
      JSON.stringify({
        jsonrpc: "2.0",
        id: 3,
        method: "session_send_event",
        params: { event: { type: "terminal.resize", cols: 120, rows: 40 } },
      }),
    )
    const resizeResponse = (await messages.next()) as any
    expect(resizeResponse.result.ok).toBe(true)
    expect(resizeResponse.result.normalizedEvent).toEqual({
      type: "terminal.resize",
      cols: 120,
      rows: 40,
    })
    expect(driver.sendEvent).toHaveBeenCalledWith({ type: "terminal.resize", cols: 120, rows: 40 })

    driver.emit({ type: "output.terminal", data: "hello" })
    const outputNotification = (await messages.next()) as any
    expect(outputNotification.method).toBe("session_event")
    expect(outputNotification.params.event.type).toBe("output.terminal")
    expect(outputNotification.params.event.data).toBe("hello")
    expect(outputNotification.params.sequence).toBe(1)

    const terminalSnapshotNotification = (await messages.next()) as any
    expect(terminalSnapshotNotification.method).toBe("session_event")
    expect(terminalSnapshotNotification.params.event.type).toBe("output.normalized")
    expect(terminalSnapshotNotification.params.event.payload.kind).toBe("terminal")
    expect(terminalSnapshotNotification.params.event.payload.source.format).toBe("terminal")
    expect(terminalSnapshotNotification.params.sequence).toBe(2)

    const payload: NormalizedSessionPayload = {
      schemaVersion: 1,
      source: { driver: "codex", format: "json-line" },
      kind: "delta",
      text: "world",
      raw: { type: "text_delta", text: "world" },
    }
    driver.emit({ type: "output.normalized", payload })
    const normalizedNotification = (await messages.next()) as any
    expect(normalizedNotification.method).toBe("session_event")
    expect(normalizedNotification.params.event.type).toBe("output.normalized")
    expect(normalizedNotification.params.event.payload.kind).toBe("delta")
    expect(normalizedNotification.params.event.payload.text).toBe("world")
    expect(normalizedNotification.params.sequence).toBe(3)

    ws.send(JSON.stringify({ jsonrpc: "2.0", id: 4, method: "session_get_state" }))
    const screenResponse = (await messages.next()) as any
    expect(screenResponse.result.terminal.cols).toBe(120)
    expect(screenResponse.result.terminal.rows).toBe(40)
    expect(Array.isArray(screenResponse.result.terminal.lines)).toBe(true)

    ws.close()
    await server.close()
  })
})
