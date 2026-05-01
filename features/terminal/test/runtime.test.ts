import { expect, test } from "bun:test"

import { runTerminalRuntimeCheck } from "../src/daemon/self-test.ts"

test("daemon terminal manager can spawn, write, resize, and close a PTY", async () => {
  const result = await runTerminalRuntimeCheck()

  expect(result.ok).toBe(true)
  expect(result.output).toContain(result.marker)
  expect(result.events).toContain("terminal.created")
  expect(result.events).toContain("terminal.output")
  expect(result.events).toContain("terminal.exit")
  expect(result.remainingTerminals).toBe(0)
})
