import { Buffer } from "node:buffer"
import type { TerminalDaemonEvent } from "@goddard-ai/schema/daemon/terminals"

const MAX_BUFFERED_EVENT_BYTES = 1024 * 1024

type QueuedTerminalEvent = {
  event: TerminalDaemonEvent
  bytes: number
}

/** Fixed-size event queue that fails closed instead of dropping terminal bytes. */
export class TerminalEventQueue {
  readonly #events: QueuedTerminalEvent[] = []
  #bufferedBytes = 0
  #overflowed = false

  get overflowed() {
    return this.#overflowed
  }

  push(event: TerminalDaemonEvent) {
    if (this.#overflowed) {
      return false
    }

    const bytes = Buffer.byteLength(JSON.stringify(event)) + 1
    if (this.#bufferedBytes + bytes > MAX_BUFFERED_EVENT_BYTES) {
      this.#overflowed = true
      return false
    }

    this.#events.push({ event, bytes })
    this.#bufferedBytes += bytes
    return true
  }

  shift() {
    const queued = this.#events.shift()
    if (!queued) {
      return undefined
    }

    this.#bufferedBytes -= queued.bytes
    return queued.event
  }
}
