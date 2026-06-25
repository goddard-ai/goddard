import { chmod, mkdtemp, rm, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { delimiter, join } from "node:path"
import { afterEach, expect, test } from "bun:test"

import { resolveSpawnSpec } from "../src/daemon/worktrees/process.ts"

const windowsTest = process.platform === "win32" ? test : test.skip
const originalPath = process.env.PATH
const originalPathExt = process.env.PATHEXT
const originalComSpec = process.env.ComSpec
const cleanup: string[] = []

afterEach(async () => {
  restoreEnv("PATH", originalPath)
  restoreEnv("PATHEXT", originalPathExt)
  restoreEnv("ComSpec", originalComSpec)

  while (cleanup.length > 0) {
    await rm(cleanup.pop()!, { recursive: true, force: true })
  }
})

windowsTest("windows spawn resolution leaves exe commands for spawn to resolve", async () => {
  const binDir = await createWindowsExecutable("git.EXE")
  process.env.PATH = `${binDir}${delimiter}${originalPath ?? ""}`
  process.env.PATHEXT = ".COM;.EXE;.BAT;.CMD"

  const spawnSpec = await resolveSpawnSpec("git", ["status"])

  expect(spawnSpec).toEqual({
    command: "git",
    args: ["status"],
  })
})

windowsTest("windows spawn resolution runs batch commands through cmd", async () => {
  const binDir = await createWindowsExecutable("pnpm.CMD")
  process.env.PATH = `${binDir}${delimiter}${originalPath ?? ""}`
  process.env.PATHEXT = ".COM;.EXE;.BAT;.CMD"
  process.env.ComSpec = "C:\\Windows\\System32\\cmd.exe"

  const spawnSpec = await resolveSpawnSpec("pnpm", ["install", "--frozen-lockfile"])

  expect(spawnSpec.command).toBe("C:\\Windows\\System32\\cmd.exe")
  expect(spawnSpec.args).toEqual([
    "/d",
    "/s",
    "/c",
    `${join(binDir, "pnpm.CMD")} install --frozen-lockfile`,
  ])
})

async function createWindowsExecutable(name: string) {
  const binDir = await mkdtemp(join(tmpdir(), "goddard-process-bin-"))
  cleanup.push(binDir)

  const executablePath = join(binDir, name)
  await writeFile(executablePath, "", "utf-8")
  await chmod(executablePath, 0o755)

  return binDir
}

function restoreEnv(name: string, value: string | undefined) {
  if (value === undefined) {
    delete process.env[name]
    return
  }

  process.env[name] = value
}
