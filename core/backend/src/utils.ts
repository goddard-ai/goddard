import type { AuthSession } from "@goddard-ai/schema/backend"

import type { SessionRecord, StreamSink } from "./api/in-memory-control-plane.ts"

export function hashToInteger(value: string): number {
  let hash = 0
  for (let i = 0; i < value.length; i += 1) {
    hash = (hash << 5) - hash + value.charCodeAt(i)
    hash |= 0
  }
  return Math.abs(hash) + 1000
}

export function toPublicSession(session: SessionRecord): AuthSession {
  return {
    token: session.token,
    githubUsername: session.githubUsername,
    githubUserId: session.githubUserId,
  }
}

export function createSseSession(onClose: () => void): { response: Response; sink: StreamSink } {
  const encoder = new TextEncoder()
  let controller: ReadableStreamDefaultController<Uint8Array> | null = null
  let isClosed = false

  const close = () => {
    if (isClosed) {
      return
    }

    isClosed = true
    try {
      controller?.close()
    } catch {
      // no-op: controller can already be closed by the runtime
    }
    onClose()
  }

  const stream = new ReadableStream<Uint8Array>({
    start(ctrl) {
      controller = ctrl
      ctrl.enqueue(encoder.encode(": connected\n\n"))
    },
    cancel() {
      close()
    },
  })

  const sink: StreamSink = {
    send(payload) {
      if (isClosed || !controller) {
        return
      }

      controller.enqueue(encoder.encode(formatSseDataFrame(payload)))
    },
    close,
  }

  return {
    response: new Response(stream, {
      status: 200,
      headers: {
        "content-type": "text/event-stream",
        "cache-control": "no-cache, no-transform",
        connection: "keep-alive",
      },
    }),
    sink,
  }
}

export function formatSseDataFrame(payload: string): string {
  const normalized = payload.replace(/\r\n/g, "\n").replace(/\r/g, "\n")
  const lines = normalized.split("\n")
  return `${lines.map((line) => `data: ${line}`).join("\n")}\n\n`
}
