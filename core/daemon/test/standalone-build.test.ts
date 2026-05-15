import { spawnSync } from "node:child_process"
import { mkdtemp, readFile, rm, stat } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join, resolve } from "node:path"
import { afterEach, expect, test } from "bun:test"

const cleanupDirs: string[] = []
const packageDir = resolve(import.meta.dirname, "..")

afterEach(async () => {
  while (cleanupDirs.length > 0) {
    const directory = cleanupDirs.pop()
    if (directory) {
      await rm(directory, { recursive: true, force: true })
    }
  }
})

test("build-standalone emits compiled daemon and helper executables with a manifest", async () => {
  const outputDir = await mkdtemp(join(tmpdir(), "goddard-daemon-standalone-"))
  cleanupDirs.push(outputDir)
  const target = resolveCurrentBunTarget()
  const executableExt = process.platform === "win32" ? ".exe" : ""
  const result = spawnSync(
    process.execPath,
    [
      join(packageDir, "scripts", "build-standalone.ts"),
      "--target",
      target,
      "--out-dir",
      outputDir,
    ],
    {
      cwd: packageDir,
      stdio: "inherit",
      env: process.env,
    },
  )

  expect(result.status).toBe(0)

  const [daemonStat, goddardStat, workforceStat, manifest] = await Promise.all([
    stat(join(outputDir, "bin", `goddard-daemon${executableExt}`)),
    stat(join(outputDir, "agent-bin", `goddard${executableExt}`)),
    stat(join(outputDir, "agent-bin", `workforce${executableExt}`)),
    readFile(join(outputDir, "manifest.json"), "utf8").then((source) => JSON.parse(source)),
  ])

  expect(daemonStat.isFile()).toBe(true)
  expect(goddardStat.isFile()).toBe(true)
  expect(workforceStat.isFile()).toBe(true)
  expect(manifest).toMatchObject({
    formatVersion: 1,
    target,
    version: "0.1.0",
    executablePath: `bin/goddard-daemon${executableExt}`,
    agentBinDir: "agent-bin",
    helperPaths: {
      goddard: `agent-bin/goddard${executableExt}`,
      workforce: `agent-bin/workforce${executableExt}`,
    },
  })
  expect(typeof manifest.runtimeHash).toBe("string")
  expect(manifest.runtimeHash.length).toBeGreaterThan(0)
})

test("build-standalone can emit helper launchers for a shared Bun runtime", async () => {
  const outputDir = await mkdtemp(join(tmpdir(), "goddard-daemon-shared-bun-"))
  cleanupDirs.push(outputDir)
  const target = resolveCurrentBunTarget()
  const executableExt = process.platform === "win32" ? ".exe" : ""
  const result = spawnSync(
    process.execPath,
    [
      join(packageDir, "scripts", "build-standalone.ts"),
      "--target",
      target,
      "--out-dir",
      outputDir,
      "--helper-runtime",
      "shared-bun",
    ],
    {
      cwd: packageDir,
      stdio: "inherit",
      env: process.env,
    },
  )

  expect(result.status).toBe(0)

  const [daemonStat, goddardStat, workforceStat, goddardPayloadStat, workforcePayloadStat] =
    await Promise.all([
      stat(join(outputDir, "bin", `goddard-daemon${executableExt}`)),
      stat(join(outputDir, "agent-bin", "goddard")),
      stat(join(outputDir, "agent-bin", "workforce")),
      stat(join(outputDir, "agent-bin", "goddard.mjs")),
      stat(join(outputDir, "agent-bin", "workforce.mjs")),
    ])
  const [goddardLauncher, workforceLauncher, manifest] = await Promise.all([
    readFile(join(outputDir, "agent-bin", "goddard"), "utf8"),
    readFile(join(outputDir, "agent-bin", "workforce"), "utf8"),
    readFile(join(outputDir, "manifest.json"), "utf8").then((source) => JSON.parse(source)),
  ])

  expect(daemonStat.isFile()).toBe(true)
  expect(goddardStat.isFile()).toBe(true)
  expect(workforceStat.isFile()).toBe(true)
  expect(goddardPayloadStat.isFile()).toBe(true)
  expect(workforcePayloadStat.isFile()).toBe(true)
  expect(goddardLauncher).toContain("${GODDARD_BUN_RUNTIME:-bun}")
  expect(workforceLauncher).toContain("${GODDARD_BUN_RUNTIME:-bun}")
  expect(manifest).toMatchObject({
    formatVersion: 1,
    target,
    version: "0.1.0",
    executablePath: `bin/goddard-daemon${executableExt}`,
    agentBinDir: "agent-bin",
    helperPaths: {
      goddard: "agent-bin/goddard",
      workforce: "agent-bin/workforce",
    },
  })
  expect(typeof manifest.runtimeHash).toBe("string")
  expect(manifest.runtimeHash.length).toBeGreaterThan(0)
})

test("build-standalone can emit all commands as launchers for a shared Bun runtime", async () => {
  const outputDir = await mkdtemp(join(tmpdir(), "goddard-daemon-shared-runtime-"))
  cleanupDirs.push(outputDir)
  const target = resolveCurrentBunTarget()
  const result = spawnSync(
    process.execPath,
    [
      join(packageDir, "scripts", "build-standalone.ts"),
      "--target",
      target,
      "--out-dir",
      outputDir,
      "--runtime",
      "shared-bun",
    ],
    {
      cwd: packageDir,
      stdio: "inherit",
      env: process.env,
    },
  )

  expect(result.status).toBe(0)

  const [
    daemonStat,
    daemonPayloadStat,
    goddardStat,
    goddardPayloadStat,
    workforceStat,
    workforcePayloadStat,
  ] = await Promise.all([
    stat(join(outputDir, "bin", "goddard-daemon")),
    stat(join(outputDir, "bin", "goddard-daemon.mjs")),
    stat(join(outputDir, "agent-bin", "goddard")),
    stat(join(outputDir, "agent-bin", "goddard.mjs")),
    stat(join(outputDir, "agent-bin", "workforce")),
    stat(join(outputDir, "agent-bin", "workforce.mjs")),
  ])
  const [daemonLauncher, manifest] = await Promise.all([
    readFile(join(outputDir, "bin", "goddard-daemon"), "utf8"),
    readFile(join(outputDir, "manifest.json"), "utf8").then((source) => JSON.parse(source)),
  ])

  expect(daemonStat.isFile()).toBe(true)
  expect(daemonPayloadStat.isFile()).toBe(true)
  expect(goddardStat.isFile()).toBe(true)
  expect(goddardPayloadStat.isFile()).toBe(true)
  expect(workforceStat.isFile()).toBe(true)
  expect(workforcePayloadStat.isFile()).toBe(true)
  expect(daemonLauncher).toContain("${GODDARD_BUN_RUNTIME:-bun}")
  expect(manifest).toMatchObject({
    formatVersion: 1,
    target,
    version: "0.1.0",
    executablePath: "bin/goddard-daemon",
    agentBinDir: "agent-bin",
    helperPaths: {
      goddard: "agent-bin/goddard",
      workforce: "agent-bin/workforce",
    },
  })
  expect(typeof manifest.runtimeHash).toBe("string")
  expect(manifest.runtimeHash.length).toBeGreaterThan(0)
})

/** Returns the Bun compile target string for the current test host. */
function resolveCurrentBunTarget() {
  const os =
    process.platform === "darwin"
      ? "darwin"
      : process.platform === "win32"
        ? "windows"
        : process.platform

  return `bun-${os}-${process.arch}`
}
