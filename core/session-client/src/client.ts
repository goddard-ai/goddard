import { JSONRPCClient, type JSONRPCRequest } from "json-rpc-2.0"
import WebSocket from "ws"

import { createClientUrl, type ServerEndpoint } from "./transport"

export interface ClientOptions {
  endpoint: ServerEndpoint
  refreshRateMs?: number
}

export function startClient(options: ClientOptions) {
  // The client is intentionally dumb about discovery: callers must provide the
  // exact endpoint returned by the server for the target session.
  const url = createClientUrl(options.endpoint)
  const refreshRateMs = options.refreshRateMs || 100

  const ws = new WebSocket(url)

  // The JSON-RPC client only needs a transport callback because the request ids
  // are handled internally by the library for this simple request/notify flow.
  const rpcClient = new JSONRPCClient((request: JSONRPCRequest) => {
    if (ws.readyState === WebSocket.OPEN) {
      ws.send(JSON.stringify(request))
      return Promise.resolve()
    }
    return Promise.reject(new Error("WebSocket is not open"))
  })

  ws.on("message", (data) => {
    rpcClient.receive(JSON.parse(data.toString()))
  })

  ws.on("open", () => {
    // Raw mode lets us forward keystrokes byte-for-byte to the remote PTY,
    // including control characters and escape sequences.
    process.stdin.setRawMode(true)
    process.stdin.resume()
    process.stdin.setEncoding("utf8")

    process.stdin.on("data", (key: string) => {
      if (key === "\u0003") {
        process.stdout.write("\x1B[2J\x1B[0f")
        process.exit()
      }

      rpcClient.notify("rpc_write_input", { key })
    })

    // This client uses polling instead of a push render stream so it can stay
    // transport-agnostic and keep the protocol surface minimal.
    setInterval(async () => {
      try {
        const screenLines = (await rpcClient.request("rpc_get_screen_state", {})) as string[]
        process.stdout.write("\x1B[2J\x1B[0f")
        process.stdout.write(screenLines.join("\n"))
      } catch {
        // Ignore transient polling errors.
      }
    }, refreshRateMs)
  })

  ws.on("close", () => {
    console.log("Disconnected from server")
    process.exit(0)
  })

  ws.on("error", (error: Error) => {
    console.error("WebSocket error:", error)
    process.exit(1)
  })

  return {
    ws,
    rpcClient,
    close: () => {
      ws.close()
    },
  }
}
