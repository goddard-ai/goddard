import { expect, test } from "bun:test"

import { DaemonTerminalProcessService } from "../src/daemon/runtime.ts"
import { runTerminalRuntimeCheck } from "../src/daemon/self-test.ts"

test("daemon terminal connection can spawn, write, resize, and close a PTY", async () => {
  const result = await runTerminalRuntimeCheck()

  expect(result.ok).toBe(true)
  expect(result.output).toContain(result.marker)
  expect(result.events).toContain("terminal.created")
  expect(result.events).toContain("terminal.output")
  expect(result.events).toContain("terminal.exit")
  expect(result.remainingTerminals).toBe(0)
})

test("daemon terminal process service exposes PTY output and exit state", async () => {
  const service = new DaemonTerminalProcessService()
  const output: string[] = []
  const marker = "__goddard_terminal_service_ok__"
  const handle = service.spawn({
    options: {
      command: process.platform === "win32" ? "cmd.exe" : "/bin/sh",
      args:
        process.platform === "win32"
          ? ["/d", "/s", "/c", `echo ${marker}`]
          : ["-c", `printf '${marker}\\n'`],
    },
    onOutput(data) {
      output.push(data)
    },
  })

  const result = await handle.exit

  expect(result.exitCode).toBe(0)
  expect(output.join("")).toContain(marker)
  expect(service.size).toBe(0)
})
