import { mkdtemp, rm } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { fileURLToPath } from "node:url"
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

test("formats log entries as timeline fields, message, and properties", () => {
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
    "1 2026-06-16T12:00:00.000Z daemon info ipc.response_sent pid=123 method=session.history durationMs=38 response={obj_01K00000000000000000000000}",
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

  const page = await runCli(["page", "--property", "method=session.history"], testHome)
  expect(page).toBe(
    `${row.id} ${row.at} daemon info ipc.response_sent pid=123 method=session.history response={${row.properties.response}}\n`,
  )

  const expanded = await runCli(["expand", row.properties.response as string], testHome)
  expect(expanded).toContain("response payload that collapses")
})

test("CLI help lists log subcommands", async () => {
  testHome = await mkdtemp(join(tmpdir(), "goddard-logs-cli-test-"))
  process.env.HOME = testHome

  const help = await runCli(["--help"], testHome)
  expect(help).toContain("page")
  expect(help).toContain("tail")
  expect(help).toContain("expand")
  expect(help).toContain("path")
})

async function runCli(args: string[], home: string) {
  const env: Record<string, string | undefined> = process.env

  const result = await Bun.$`bun run ./src/cli.ts ${args}`
    .cwd(fileURLToPath(new URL("..", import.meta.url)))
    .env({
      ...env,
      HOME: home,
      // Bun shell command lookup expects PATH, while Windows runners may expose Path.
      PATH: env.PATH ?? env.Path,
    })
    .quiet()
    .nothrow()
  const stdout = result.stdout.toString()
  const stderr = result.stderr.toString()
  const exitCode = result.exitCode
  expect(stderr).toBe("")
  expect(exitCode).toBe(0)
  return stdout
}
