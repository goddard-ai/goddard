import { Readable, Writable } from "node:stream"
import * as acp from "acp-client/protocol"

export type AnyRequest = acp.AnyMessage & { params: unknown }

/** Minimal Bun process stdin contract needed for ACP framing. */
type BunWritablePipe = {
  write(chunk: Uint8Array): unknown
  end(): unknown
}

/** Agent stdin shape supported across Node-compatible and Bun-native subprocesses. */
export type AgentInputStream = Writable | BunWritablePipe

/** Agent stdout shape supported across Node-compatible and Bun-native subprocesses. */
export type AgentOutputStream = Readable | ReadableStream<Uint8Array>

/** Optional callbacks used to observe raw agent stream traffic. */
export type AgentStreamHooks = {
  debug?: (event: string, fields?: Record<string, unknown>) => void
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

export function getAcpMessageResult<T>(message: acp.AnyMessage & { result: unknown }): T
export function getAcpMessageResult<T>(message: acp.AnyMessage): T | null
export function getAcpMessageResult<T>(message: acp.AnyMessage): T | null {
  return "result" in message ? (message.result as T) : null
}

export function createAgentConnection(
  stdin: AgentInputStream,
  stdout: AgentOutputStream,
  hooks: AgentStreamHooks = {},
) {
  const stream = createAgentMessageStream(stdin, stdout, hooks)

  return {
    getWriter() {
      return stream.writable.getWriter()
    },
    subscribe(onMessage: (message: acp.AnyMessage) => Promise<void>) {
      const reader = stream.readable.getReader()
      hooks.debug?.("session.acp.subscription_attached")

      const closed = (async () => {
        while (true) {
          const { value, done } = await reader.read()
          if (done) {
            hooks.debug?.("session.acp.subscription_stream_done")
            return reader.closed
          }
          hooks.debug?.("session.acp.message_dispatched", {
            hasId: "id" in value && value.id != null,
            method: "method" in value ? value.method : undefined,
          })
          onMessage(value).catch((error) => {
            hooks.debug?.("session.acp.message_handler_failed", {
              errorMessage: error instanceof Error ? error.message : String(error),
            })
            if (hooks.onMessageError) {
              hooks.onMessageError(error)
              return
            }

            throw error
          })
        }
      })()

      return {
        closed,
        async close() {
          hooks.debug?.("session.acp.subscription_close_requested")
          await reader.cancel()
        },
      }
    },
  }
}

export function createAgentMessageStream(
  stdin: AgentInputStream,
  stdout: AgentOutputStream,
  hooks: AgentStreamHooks = {},
) {
  const readable = toReadableStream(stdout)
  const instrumentedReadable =
    hooks.onChunk || hooks.debug
      ? readable.pipeThrough(
          new TransformStream<Uint8Array, Uint8Array>({
            transform(chunk, controller) {
              hooks.onChunk?.(chunk)
              hooks.debug?.("session.acp.chunk_read", {
                byteLength: chunk.byteLength,
              })
              controller.enqueue(chunk)
            },
          }) as unknown as ReadableWritablePair<Uint8Array, Uint8Array>,
        )
      : readable

  return acp.ndJsonStream(toWritableStream(stdin, hooks), instrumentedReadable)
}

/** Normalizes agent stdin into a web writable stream for ACP NDJSON framing. */
function toWritableStream(
  input: AgentInputStream,
  hooks: AgentStreamHooks = {},
): WritableStream<Uint8Array> {
  if (input instanceof Writable) {
    const writable = Writable.toWeb(input)
    const writer = writable.getWriter()
    return new WritableStream<Uint8Array>({
      async write(chunk) {
        hooks.debug?.("session.acp.chunk_write", {
          byteLength: chunk.byteLength,
        })
        await writer.write(chunk)
      },
      async close() {
        hooks.debug?.("session.acp.input_closed")
        await writer.close()
      },
      async abort(reason) {
        hooks.debug?.("session.acp.input_aborted")
        await writer.abort(reason)
      },
    })
  }

  return new WritableStream<Uint8Array>({
    async write(chunk) {
      hooks.debug?.("session.acp.chunk_write", {
        byteLength: chunk.byteLength,
      })
      await Promise.resolve(input.write(chunk))
    },
    async close() {
      hooks.debug?.("session.acp.input_closed")
      await Promise.resolve(input.end())
    },
    async abort() {
      hooks.debug?.("session.acp.input_aborted")
      await Promise.resolve(input.end())
    },
  })
}

/** Normalizes agent stdout into a web readable stream for ACP NDJSON framing. */
function toReadableStream(output: AgentOutputStream): ReadableStream<Uint8Array> {
  return output instanceof Readable
    ? (Readable.toWeb(output) as unknown as ReadableStream<Uint8Array>)
    : output
}
