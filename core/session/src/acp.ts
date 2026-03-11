import * as acp from "@agentclientprotocol/sdk"
import { Readable, Writable } from "node:stream"

export type AnyRequest = acp.AnyMessage & { params: any }

export function isAcpRequest<T extends AnyRequest>(
  message: { jsonrpc?: string },
  method: string,
): message is T {
  return message.jsonrpc === "2.0" && "method" in message && message.method === method
}

export function matchAcpRequest<T>(message: acp.AnyMessage, method: string): T | null {
  return isAcpRequest(message, method) ? (message.params as T) : null
}

export function getAcpMessageResult<T>(message: acp.AnyMessage): T | null {
  return "result" in message ? (message.result as T) : null
}

export function createAgentConnection(stdin: Writable, stdout: Readable) {
  const stream = acp.ndJsonStream(
    Writable.toWeb(stdin),
    Readable.toWeb(stdout) as ReadableStream<Uint8Array>,
  )

  return {
    getWriter() {
      return stream.writable.getWriter()
    },
    subscribe(onMessage: (message: acp.AnyMessage) => Promise<void>) {
      const reader = stream.readable.getReader()

      const closed = (async () => {
        while (true) {
          const { value, done } = await reader.read()
          if (done) {
            return reader.closed
          }
          onMessage(value).catch(console.error)
        }
      })()

      return {
        closed,
        async close() {
          await reader.cancel()
        },
      }
    },
  }
}
