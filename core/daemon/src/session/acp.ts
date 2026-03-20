import * as acp from "@agentclientprotocol/sdk"
import { Readable, Writable } from "node:stream"
import { TransformStream } from "node:stream/web"
import { createDaemonLogger } from "../logging.js"

export type AnyRequest = acp.AnyMessage & { params: unknown }

/** Optional callbacks used to observe raw agent stream traffic. */
export type AgentStreamHooks = {
  onChunk?: (chunk: Uint8Array) => void
  onMessageError?: (error: unknown) => void
}

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

export function createAgentConnection(
  stdin: Writable,
  stdout: Readable,
  hooks: AgentStreamHooks = {},
) {
  const logger = createDaemonLogger()
  const stream = createAgentMessageStream(stdin, stdout, hooks)

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
          onMessage(value).catch((error) => {
            if (hooks.onMessageError) {
              hooks.onMessageError(error)
              return
            }

            logger.log("agent.message_handler_failed", {
              errorMessage: error instanceof Error ? error.message : String(error),
            })
          })
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

export function createAgentMessageStream(
  stdin: Writable,
  stdout: Readable,
  hooks: AgentStreamHooks = {},
) {
  const readable = Readable.toWeb(stdout) as ReadableStream<Uint8Array>
  const instrumentedReadable = hooks.onChunk
    ? readable.pipeThrough(
        new TransformStream<Uint8Array, Uint8Array>({
          transform(chunk, controller) {
            hooks.onChunk?.(chunk)
            controller.enqueue(chunk)
          },
        }) as ReadableWritablePair<Uint8Array, Uint8Array>,
      )
    : readable

  return acp.ndJsonStream(Writable.toWeb(stdin), instrumentedReadable)
}
