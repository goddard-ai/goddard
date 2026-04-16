import { afterEach, expect, test } from "bun:test"
import { spawnSync } from "node:child_process"
import { existsSync } from "node:fs"
import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join } from "node:path"

import { createACPRegistryService } from "../src/session/registry.ts"

const cleanupDirs: string[] = []

afterEach(async () => {
  while (cleanupDirs.length > 0) {
    await rm(cleanupDirs.pop()!, { recursive: true, force: true })
  }
})

test("registry service clones and refreshes the cached registry catalog", async () => {
  const remoteDir = await createRegistryRepo("0.0.25")
  const cacheDir = await mkdtemp(join(tmpdir(), "goddard-registry-cache-"))
  cleanupDirs.push(cacheDir)
  let nowMs = Date.parse("2026-04-11T00:00:00.000Z")
  const service = createACPRegistryService({
    cacheDir,
    registryUrl: remoteDir,
    now: () => nowMs,
    cloneTtlMs: 60_000,
  })

  const firstSnapshot = await service.listAdapters()

  expect(firstSnapshot.registrySource).toBe("cache")
  expect(firstSnapshot.stale).toBe(false)
  expect(firstSnapshot.adapters[0]).toMatchObject({
    id: "pi-acp",
    version: "0.0.25",
    unofficial: true,
  })
  expect(existsSync(join(cacheDir, ".git"))).toBe(true)

  await writeRegistryAgent(remoteDir, "0.0.26")
  runGit(remoteDir, ["add", "."])
  runGit(remoteDir, ["commit", "-m", "update"])

  nowMs += 61_000

  const secondSnapshot = await service.listAdapters()

  expect(secondSnapshot.registrySource).toBe("cache")
  expect(secondSnapshot.stale).toBe(false)
  expect(secondSnapshot.adapters[0]?.version).toBe("0.0.26")
})

test("registry service falls back to the bundled snapshot when the initial clone fails", async () => {
  const cacheDir = await mkdtemp(join(tmpdir(), "goddard-registry-cache-"))
  cleanupDirs.push(cacheDir)
  const service = createACPRegistryService({
    cacheDir,
    registryUrl: join(tmpdir(), "missing-registry-repo"),
  })

  const snapshot = await service.listAdapters()

  expect(snapshot.registrySource).toBe("fallback")
  expect(snapshot.stale).toBe(true)
  expect(snapshot.lastError).toBeTruthy()
  expect(snapshot.adapters.length).toBeGreaterThan(0)
  expect(existsSync(join(cacheDir, ".git"))).toBe(false)
})

test("registry service preserves the cached clone when refresh fails", async () => {
  const remoteDir = await createRegistryRepo("0.0.25")
  const cacheDir = await mkdtemp(join(tmpdir(), "goddard-registry-cache-"))
  cleanupDirs.push(cacheDir)
  let nowMs = Date.parse("2026-04-11T00:00:00.000Z")
  const service = createACPRegistryService({
    cacheDir,
    registryUrl: remoteDir,
    now: () => nowMs,
    cloneTtlMs: 60_000,
  })

  await service.listAdapters()
  await rm(remoteDir, { recursive: true, force: true })
  nowMs += 61_000

  const snapshot = await service.listAdapters()

  expect(snapshot.registrySource).toBe("cache")
  expect(snapshot.stale).toBe(true)
  expect(snapshot.lastError).toBeTruthy()
  expect(snapshot.adapters[0]?.version).toBe("0.0.25")
})

async function createRegistryRepo(version: string) {
  const repoDir = await mkdtemp(join(tmpdir(), "goddard-registry-remote-"))
  cleanupDirs.push(repoDir)
  await writeRegistryAgent(repoDir, version)
  runGit(repoDir, ["init"])
  runGit(repoDir, ["config", "user.email", "bot@example.com"])
  runGit(repoDir, ["config", "user.name", "Bot"])
  runGit(repoDir, ["add", "."])
  runGit(repoDir, ["commit", "-m", "init"])
  return repoDir
}

async function writeRegistryAgent(repoDir: string, version: string) {
  const agentDir = join(repoDir, "pi-acp")
  await mkdir(agentDir, { recursive: true })
  await writeFile(
    join(agentDir, "agent.json"),
    JSON.stringify(
      {
        id: "pi-acp",
        name: "pi ACP",
        version,
        description: "ACP adapter for pi coding agent",
        repository: "https://github.com/svkozak/pi-acp",
        authors: ["Sergii Kozak <svkozak@gmail.com>"],
        license: "MIT",
        distribution: {
          npx: {
            package: `pi-acp@${version}`,
          },
        },
      },
      null,
      2,
    ),
    "utf8",
  )
  await writeFile(
    join(agentDir, "icon.svg"),
    `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><text x="1" y="12">pi</text></svg>`,
    "utf8",
  )
}

function runGit(cwd: string, args: string[]) {
  const result = spawnSync("git", args, {
    cwd,
    encoding: "utf8",
  })

  expect(result.status).toBe(0)
}
