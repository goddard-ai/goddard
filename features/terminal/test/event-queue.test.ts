import type { TerminalDaemonEvent } from "@goddard-ai/schema/daemon/terminals"
import { expect, test } from "bun:test"

import { TerminalEventQueue } from "../src/daemon/event-queue.ts"

function output(data: string): TerminalDaemonEvent {
  return {
    type: "terminal.output",
    connectionId: "connection-1",
    instanceId: "terminal-1",
    data,
  }
}

test("terminal event queue preserves order within its fixed buffer", () => {
  const queue = new TerminalEventQueue()
  const first = output("first")
  const second = output("second")

  expect(queue.push(first)).toBe(true)
  expect(queue.push(second)).toBe(true)
  expect(queue.shift()).toEqual(first)
  expect(queue.shift()).toEqual(second)
  expect(queue.shift()).toBeUndefined()
  expect(queue.overflowed).toBe(false)
})

test("terminal event queue fails closed instead of dropping bytes", () => {
  const queue = new TerminalEventQueue()

  expect(queue.push(output("x".repeat(1024 * 1024)))).toBe(false)
  expect(queue.overflowed).toBe(true)
  expect(queue.push(output("later"))).toBe(false)
})
