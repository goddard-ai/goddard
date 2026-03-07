import { rmSync } from "node:fs"
import { createServer } from "node:http"
import type { AddressInfo } from "node:net"

import XtermHeadless from "@xterm/headless"
import { JSONRPCServer } from "json-rpc-2.0"
import * as pty from "node-pty"
import { WebSocketServer } from "ws"
import type { WebSocket } from "ws"

import type { SessionDriver } from "./drivers/types.ts"
import { createServerEndpoint, resolveServerListenTarget } from "./transport.ts"

export interface ServerOptions {
  transport?: "tcp" | "ipc"
  port?: number
  socketPath?: string
  command?: string
  args?: string[]
  cwd?: string
  driver?: SessionDriver
}

function createDefaultPtyDriver(options: Pick<ServerOptions, "command" | "args" | "cwd">): SessionDriver {
  const command = options.command || "bash"
  const args = options.args || []
  const cwd = options.cwd || process.cwd()

  const ptyProcess = pty.spawn(command, args, {
    name: "xterm-color",
    cols: 80,
    rows: 24,
    cwd,
    env: process.env as Record<string, string>,
  })

  return {
    name: "pty",
    writeInput: (key: string) => {
      ptyProcess.write(key)
    },
    onData: (listener) => {
      const disposable = ptyProcess.onData((data) => listener(data))
      return () => disposable.dispose()
    },
    close: () => {
      ptyProcess.kill()
    },
  }
}

export async function startServer(options: ServerOptions = {}) {
  const listenTarget = resolveServerListenTarget(options)

  const driver = options.driver ?? createDefaultPtyDriver(options)
  if (!driver.writeInput || !driver.onData) {
    throw new Error("startServer requires a driver with writeInput and onData methods")
  }

  const headlessTerm = new XtermHeadless.Terminal({
    cols: 80,
    rows: 24,
    allowProposedApi: true,
  })

  const unsubscribeFromDriver = driver.onData((data) => {
    headlessTerm.write(data)
  })

  const rpcServer = new JSONRPCServer()

  rpcServer.addMethod("rpc_write_input", (params) => {
    const { key } = params as { key: string }
    if (key) {
      driver.writeInput?.(key)
    }
  })

  rpcServer.addMethod("rpc_get_screen_state", () => {
    const buffer = headlessTerm.buffer.active
    const lines: string[] = []

    for (let i = 0; i < buffer.length; i++) {
      const line = buffer.getLine(i)
      lines.push(line ? line.translateToString(true) : "")
    }

    return lines
  })

  const httpServer = createServer()
  const wss = new WebSocketServer({ server: httpServer })

  wss.on("connection", (ws: WebSocket) => {
    ws.on("message", (message) => {
      rpcServer.receive(JSON.parse(message.toString())).then((response) => {
        if (response) {
          ws.send(JSON.stringify(response))
        }
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
