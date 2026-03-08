import { JSONRPCClient, type JSONRPCRequest } from "json-rpc-2.0"
import type {
  NormalizedSessionPayload,
  SessionDriverCapabilities,
  SessionEndpoint,
  SessionEventNotification,
  SessionInitializeResult,
} from "@goddard-ai/session-protocol"
import WebSocket from "ws"

import { createClientUrl } from "./transport.ts"

export interface ClientOptions {
  endpoint: SessionEndpoint
  onNormalizedPayload?: (payload: NormalizedSessionPayload) => void
  stdin?: NodeJS.ReadStream
  stdout?: NodeJS.WriteStream
  stderr?: NodeJS.WriteStream
}

function isSessionEventNotification(payload: unknown): payload is SessionEventNotification {
  return (
    typeof payload === "object" &&
    payload !== null &&
    "method" in payload &&
    (payload as { method?: unknown }).method === "session_event" &&
    "params" in payload
  )
}

function canUseTerminalInput(capabilities: SessionDriverCapabilities): boolean {
  return capabilities.terminal.enabled
}

export function startClient(options: ClientOptions) {
  const stdin = options.stdin ?? process.stdin
  const stdout = options.stdout ?? process.stdout
  const stderr = options.stderr ?? process.stderr

  const url = createClientUrl(options.endpoint)
  const ws = new WebSocket(url)

  const rpcClient = new JSONRPCClient((request: JSONRPCRequest) => {
    if (ws.readyState === WebSocket.OPEN) {
      ws.send(JSON.stringify(request))
      return Promise.resolve()
    }
    return Promise.reject(new Error("WebSocket is not open"))
  })

  let terminalMode = false

  const sendResize = async () => {
    if (!terminalMode) {
      return
    }

    const cols = stdout.columns || 80
    const rows = stdout.rows || 24

    await rpcClient.request("session_send_event", {
      event: {
        type: "terminal.resize",
        cols,
        rows,
      },
    })
  }

  const onStdinData = (key: string) => {
    if (key === "\u0003") {
      process.exit(0)
    }

    rpcClient.notify("session_send_event", {
      event: {
        type: terminalMode ? "input.terminal" : "input.text",
        ...(terminalMode ? { data: key } : { text: key }),
      },
    })
  }

  const onResize = () => {
    void sendResize()
  }

  ws.on("message", (data) => {
    const payload = JSON.parse(data.toString()) as unknown

    if (isSessionEventNotification(payload)) {
      const event = payload.params.event
      switch (event.type) {
        case "output.terminal":
          stdout.write(event.data)
          return
        case "output.text":
          stdout.write(event.text)
          return
        case "output.normalized":
          options.onNormalizedPayload?.(event.payload)
          return
        case "session.error":
          stderr.write(`${event.message}\n`)
          return
        case "session.exit":
          process.exit(event.exitCode)
      }
    }

    rpcClient.receive(payload as any)
  })

  ws.on("open", async () => {
    const init = (await rpcClient.request("session_initialize")) as SessionInitializeResult
    terminalMode = canUseTerminalInput(init.capabilities)

    if (terminalMode && stdin.isTTY) {
      stdin.setRawMode(true)
      stdout.write("\x1b[?25h")
      stdout.on("resize", onResize)
      await sendResize()
    }

    stdin.resume()
    stdin.setEncoding("utf8")
    stdin.on("data", onStdinData)
  })

  ws.on("close", () => {
    if (stdin.isTTY) {
      stdin.setRawMode(false)
    }
    stdin.off("data", onStdinData)
    stdout.off("resize", onResize)
    process.exit(0)
  })

  ws.on("error", (error: Error) => {
    stderr.write(`WebSocket error: ${error.message}\n`)
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
