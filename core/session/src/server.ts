import { rmSync } from "node:fs"
import { createServer } from "node:http"
import type { AddressInfo } from "node:net"

import XtermHeadless from "@xterm/headless"
import { JSONRPCServer } from "json-rpc-2.0"
import type {
  SessionClientEvent,
  SessionDriverCapabilities,
  SessionServerEvent,
  SessionStartupInput,
  SessionTerminalState,
} from "@goddard-ai/session-protocol"
import {
  sessionClientEventSchema,
  sessionInitializeParamsSchema,
} from "@goddard-ai/session-protocol"
import { WebSocketServer } from "ws"
import type { WebSocket } from "ws"

import type { SessionDriver } from "./drivers/types.ts"
import { createPtyServerDriver } from "./drivers/pty.ts"
import { createServerEndpoint, resolveServerListenTarget } from "./transport.ts"

export interface ServerOptions {
  transport?: "tcp" | "ipc"
  port?: number
  socketPath?: string
  command?: string
  args?: string[]
  cwd?: string
  driver?: SessionDriver
  startupInput?: SessionStartupInput
}

export async function startServer(options: ServerOptions = {}) {
  const listenTarget = resolveServerListenTarget(options)

  const driver = options.driver ?? createPtyServerDriver(options)
  const capabilities: SessionDriverCapabilities = driver.getCapabilities?.() ?? {
    terminal: {
      enabled: false,
      canResize: false,
      hasScreenState: false,
    },
    normalizedOutput: false,
  }

  const headlessTerm = new XtermHeadless.Terminal({
    cols: 80,
    rows: 24,
    allowProposedApi: true,
  })

  const clients = new Set<WebSocket>()

  const sendNotification = (method: string, params: unknown) => {
    const payload = JSON.stringify({ jsonrpc: "2.0", method, params })
    for (const client of clients) {
      if (client.readyState === 1) {
        client.send(payload)
      }
    }
  }

  let eventSequence = 0
  const emitSessionEvent = (event: SessionServerEvent) => {
    eventSequence += 1
    sendNotification("session_event", {
      sequence: eventSequence,
      event,
    })
  }

  const onDriverEvent = (event: SessionServerEvent) => {
    if (event.type === "output.terminal") {
      headlessTerm.write(event.data)
      emitSessionEvent(event)
      emitSessionEvent({
        type: "output.normalized",
        payload: {
          schemaVersion: 1,
          source: {
            driver: driver.name,
            format: "terminal",
          },
          kind: "terminal",
          terminal: getTerminalState(),
          raw: {
            data: event.data,
          },
        },
      })
      return
    }

    if (event.type === "output.text") {
      headlessTerm.write(event.text)
      emitSessionEvent(event)
      return
    }

    if (event.type === "output.normalized") {
      emitSessionEvent(event)
      return
    }

    emitSessionEvent(event)
  }

  const unsubscribeFromDriver = driver.onEvent(onDriverEvent)
  await driver.start(options.startupInput ?? {})

  const rpcServer = new JSONRPCServer()

  function formatSchemaError(error: {
    issues: Array<{ path: ReadonlyArray<PropertyKey>; message: string }>
  }) {
    const [issue] = error.issues
    if (!issue) {
      return "Invalid request payload"
    }

    const path = issue.path.length > 0 ? issue.path.map(String).join(".") : "request"
    return `${path}: ${issue.message}`
  }

  function parseInitializeParams(params: unknown) {
    const result = sessionInitializeParamsSchema.safeParse(params)
    if (!result.success) {
      throw new Error(`Invalid session_initialize params: ${formatSchemaError(result.error)}`)
    }
  }

  function parseClientEvent(params: unknown): SessionClientEvent {
    const event = (params as { event?: unknown } | null | undefined)?.event
    const result = sessionClientEventSchema.safeParse(event)
    if (!result.success) {
      throw new Error(`Invalid session_send_event params: ${formatSchemaError(result.error)}`)
    }
    return result.data
  }

  function getTerminalState(): SessionTerminalState {
    const buffer = headlessTerm.buffer.active
    const lines: string[] = []

    for (let i = 0; i < buffer.length; i++) {
      const line = buffer.getLine(i)
      lines.push(line ? line.translateToString(true) : "")
    }

    return {
      cols: headlessTerm.cols,
      rows: headlessTerm.rows,
      lines,
      cursor: {
        x: buffer.cursorX,
        y: buffer.cursorY,
      },
    }
  }

  rpcServer.addMethod("session_initialize", async (params) => {
    parseInitializeParams(params)
    return {
      protocolVersion: 1,
      driver: driver.name,
      capabilities,
      state: {
        terminal: capabilities.terminal.hasScreenState ? getTerminalState() : null,
      },
    }
  })

  rpcServer.addMethod("session_send_event", async (params) => {
    const event = parseClientEvent(params)
    if (event.type === "terminal.resize") {
      if (!capabilities.terminal.canResize) {
        throw new Error("terminal.resize is not supported by this driver")
      }
      const cols = Math.max(1, Math.floor(event.cols))
      const rows = Math.max(1, Math.floor(event.rows))
      headlessTerm.resize(cols, rows)
      await driver.sendEvent({ type: "terminal.resize", cols, rows })
      return { ok: true, normalizedEvent: { type: "terminal.resize", cols, rows } }
    }

    await driver.sendEvent(event)
    return { ok: true, normalizedEvent: event }
  })

  rpcServer.addMethod("session_get_state", async () => {
    return {
      terminal: capabilities.terminal.hasScreenState ? getTerminalState() : null,
    }
  })

  const httpServer = createServer()
  const wss = new WebSocketServer({ server: httpServer })

  wss.on("connection", (ws: WebSocket) => {
    clients.add(ws)

    ws.on("close", () => {
      clients.delete(ws)
    })

    ws.on("message", (message) => {
      let payload: unknown
      try {
        payload = JSON.parse(message.toString())
      } catch {
        ws.send(
          JSON.stringify({
            jsonrpc: "2.0",
            id: null,
            error: { code: -32700, message: "Parse error" },
          }),
        )
        return
      }

      Promise.resolve(rpcServer.receive(payload as any))
        .then((response) => {
          if (response) {
            ws.send(JSON.stringify(response))
          }
        })
        .catch(() => {
          ws.send(
            JSON.stringify({
              jsonrpc: "2.0",
              id: null,
              error: { code: -32603, message: "Internal error" },
            }),
          )
        })
    })
  })

  const finalListenTarget = listenTarget

  await new Promise<void>((resolve, reject) => {
    httpServer.once("error", reject)
    httpServer.listen(finalListenTarget.value, () => {
      httpServer.off("error", reject)
      resolve()
    })
  })

  const address = httpServer.address()
  const endpoint =
    finalListenTarget.kind === "tcp"
      ? createServerEndpoint(finalListenTarget, String((address as AddressInfo).port))
      : createServerEndpoint(finalListenTarget, null)

  console.log(`Server started on ${endpoint.url}`)

  return {
    driver,
    headlessTerm,
    wss,
    httpServer,
    listenTarget: finalListenTarget,
    endpoint,
    close: async () => {
      unsubscribeFromDriver()
      wss.close()
      await new Promise<void>((resolve) => {
        httpServer.close(() => resolve())
      })
      driver.close?.()

      if (endpoint.kind === "ipc" && process.platform !== "win32") {
        try {
          rmSync(endpoint.socketPath)
        } catch {
          // Ignore cleanup races and missing socket files.
        }
      }
    },
  }
}
