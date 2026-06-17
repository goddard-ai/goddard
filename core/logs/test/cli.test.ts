import { mkdtemp, rm } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { afterEach, expect, test } from "bun:test"

import { createLogStore, formatLogEntry, type LogEntry } from "../src/index.ts"

const originalHome = process.env.HOME
let testHome: string | undefined

afterEach(async () => {
  if (testHome) {
    await rm(testHome, { recursive: true, force: true })
    testHome = undefined
  }

  if (originalHome === undefined) {
    delete process.env.HOME
  } else {
    process.env.HOME = originalHome
  }
})

test("formats log entries as scope message and properties", () => {
  const entry: LogEntry = {
    id: 1,
    at: "2026-06-16T12:00:00.000Z",
    scope: "daemon",
    level: "info",
    pid: 123,
    message: "ipc.response_sent",
    properties: {
      method: "session.history",
      durationMs: 38,
      response: "obj_01K00000000000000000000000",
    },
  }

  expect(formatLogEntry(entry)).toBe(
    "daemon ipc.response_sent method=session.history durationMs=38 response=obj_01K00000000000000000000000",
  )
})

test("CLI pages and expands logs from the canonical database", async () => {
  testHome = await mkdtemp(join(tmpdir(), "goddard-logs-cli-test-"))
  process.env.HOME = testHome
  const store = createLogStore({ inlineByteLimit: 24 })
  const row = store.append({
    scope: "daemon",
    level: "info",
    pid: 123,
    message: "ipc.response_sent",
    properties: {
      method: "session.history",
      response: "response payload that collapses",
    },
  })
  store.close()

  const page = await runCli(["--property", "method=session.history"], testHome)
  expect(page).toContain("next: pnpm goddard:logs --after-id")
  expect(page).toContain("daemon ipc.response_sent method=session.history")

  const expanded = await runCli(["expand", row.properties.response as string], testHome)
  expect(expanded).toContain("response payload that collapses")
})

async function runCli(args: string[], home: string) {
  // Windows pnpm installs can report a Bun process.execPath that does not exist.
  const bunPath = Bun.env.GODDARD_BUN_PATH ?? process.execPath
  const subprocess = Bun.spawn([bunPath, "run", "./src/cli.ts", ...args], {
    cwd: new URL("..", import.meta.url).pathname,
    env: {
      ...Bun.env,
      HOME: home,
    },
    stdout: "pipe",
    stderr: "pipe",
  })
  const [stdout, stderr, exitCode] = await Promise.all([
    new Response(subprocess.stdout).text(),
    new Response(subprocess.stderr).text(),
    subprocess.exited,
  ])
  expect(stderr).toBe("")
  expect(exitCode).toBe(0)
  return stdout
}
