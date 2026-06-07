import { mkdir, mkdtemp, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { expect, test } from "bun:test"

import { readLogSurface } from "../src/bin/goddard-tool.ts"

test("readLogSurface prints trailing lines for app logs", async () => {
  const logDir = await mkdtemp(join(tmpdir(), "goddard-tool-logs-"))
  await writeFile(join(logDir, "app.log"), ["one", "two", "three"].join("\n"), "utf-8")

  await expect(readLogSurface({ surface: "app", lines: "2", logDir })).resolves.toBe("two\nthree")
})

test("readLogSurface combines agent process stderr logs", async () => {
  const logDir = await mkdtemp(join(tmpdir(), "goddard-tool-logs-"))
  await mkdir(logDir, { recursive: true })
  await writeFile(join(logDir, "agent-process-12.stderr.log"), "first\nsecond\n", "utf-8")
  await writeFile(join(logDir, "agent-process-34.stderr.log"), "third\nfourth\n", "utf-8")

  await expect(readLogSurface({ surface: "agent-process", lines: "1", logDir })).resolves.toBe(
    [
      "== agent-process-12.stderr.log ==",
      "second",
      "== agent-process-34.stderr.log ==",
      "fourth",
    ].join("\n"),
  )
})

test("readLogSurface reports missing logs", async () => {
  const logDir = await mkdtemp(join(tmpdir(), "goddard-tool-logs-"))

  await expect(readLogSurface({ surface: "daemon", logDir })).resolves.toBe(
    `No daemon logs found in ${logDir}`,
  )
})
