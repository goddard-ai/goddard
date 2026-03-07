import { Terminal } from "@xterm/headless"
import { JSONRPCServer } from "json-rpc-2.0"
import * as pty from "node-pty"
import { WebSocketServer } from "ws"
import type { WebSocket } from "ws"

export interface ServerOptions {
  port?: number
  command?: string
  args?: string[]
  cwd?: string
}

export function startServer(options: ServerOptions = {}) {
  const port = options.port || 3000
  const command = options.command || "bash"
  const args = options.args || []
  const cwd = options.cwd || process.cwd()

  // 1. Initialize the Headless Terminal
  const headlessTerm = new Terminal({
    cols: 80,
    rows: 24,
  })

  // 2. Spawn the Agent CLI (e.g., Python REPL, Gemini CLI, bash)
  const ptyProcess = pty.spawn(command, args, {
    name: "xterm-color",
    cols: 80,
    rows: 24,
    cwd: cwd,
    env: process.env as Record<string, string>,
  })

  // 3. Pipe PTY output into the Headless Terminal
  ptyProcess.onData((data) => {
    headlessTerm.write(data)
  })

  // Set up JSON-RPC server
  const rpcServer = new JSONRPCServer()

  // Expose this to your JSON-RPC server so the client can send keystrokes
  rpcServer.addMethod("rpc_write_input", (params) => {
    const { key } = params as { key: string }
    if (key) {
      ptyProcess.write(key)
    }
  })

  // Expose this so the client can ask "What does the screen look like right now?"
  rpcServer.addMethod("rpc_get_screen_state", () => {
    const buffer = headlessTerm.buffer.active
    const lines: string[] = []

    // Extract every line as plain text, stripping out ANSI codes
    for (let i = 0; i < buffer.length; i++) {
      const line = buffer.getLine(i)
      lines.push(line ? line.translateToString(true) : "")
    }

    return lines
  })

  const wss = new WebSocketServer({ port })

  wss.on("connection", (ws: WebSocket) => {
    ws.on("message", (message) => {
      rpcServer.receive(JSON.parse(message.toString())).then((response) => {
        if (response) {
          ws.send(JSON.stringify(response))
        }
      })
    })
  })

  console.log(`Server started on ws://localhost:${port}`)

  return {
    ptyProcess,
    headlessTerm,
    wss,
    close: () => {
      wss.close()
      ptyProcess.kill()
    },
  }
}
