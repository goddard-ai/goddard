import type { AuthSession } from "@goddard-ai/auth/schema"
import { ndjson } from "@goddard-ai/backend-plugin"
import type { RepoEvent } from "@goddard-ai/remote-repo/schema"

import type { SessionRecord } from "./api/in-memory-control-plane.ts"

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
    principal: session.principal,
  }
}

export type EventQueue<T> = AsyncIterable<T> & {
  publish(value: T): void
  close(): void
}

export function createEventQueue<T>(
  filter: (value: T) => boolean = () => true,
  onClose: () => void = () => {},
): EventQueue<T> {
  const values: T[] = []
  const reads: ((result: IteratorResult<T>) => void)[] = []
  let closed = false

  const queue: EventQueue<T> = {
    publish(value) {
      if (closed || !filter(value)) {
        return
      }

      const resolve = reads.shift()
      if (resolve) {
        resolve({ done: false, value })
        return
      }

      values.push(value)
    },
    close() {
      if (closed) {
        return
      }

      closed = true
      onClose()
      for (const resolve of reads.splice(0)) {
        resolve({ done: true, value: undefined })
      }
    },
    [Symbol.asyncIterator]() {
      return {
        next: async () => {
          if (values.length > 0) {
            return { done: false, value: values.shift()! }
          }
          if (closed) {
            return { done: true, value: undefined }
          }

          return new Promise<IteratorResult<T>>((resolve) => {
            reads.push(resolve)
          })
        },
        return: async () => {
          queue.close()
          return { done: true, value: undefined }
        },
      }
    },
  }

  return queue
}

export function filterRepoEvent(
  event: RepoEvent,
  filter: { names?: readonly RepoEvent["type"][]; where?: Partial<RepoEvent> },
) {
  if (filter.names && filter.names.length > 0 && !filter.names.includes(event.type)) {
    return false
  }

  const { where } = filter
  if (!where) {
    return true
  }

  return (
    (where.owner === undefined || where.owner === event.owner) &&
    (where.repo === undefined || where.repo === event.repo) &&
    (where.prNumber === undefined || where.prNumber === event.prNumber)
  )
}

export function createReadyNdjsonResponse(source: ndjson.NdjsonSource): Response {
  const body = ndjson.encodeNdjson(source)
  const reader = body.getReader()

  return new Response(
    new ReadableStream<Uint8Array>({
      start(controller) {
        controller.enqueue(new Uint8Array())
      },
      async pull(controller) {
        const next = await reader.read()
        if (next.done) {
          controller.close()
          return
        }

        controller.enqueue(next.value)
      },
      async cancel(reason) {
        await reader.cancel(reason).catch(() => {})
      },
    }),
    {
      headers: {
        "content-type": "application/x-ndjson; charset=utf-8",
      },
    },
  )
}
