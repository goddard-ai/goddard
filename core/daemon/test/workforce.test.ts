import { spawnSync } from "node:child_process"
import { mkdir, mkdtemp, readFile, realpath, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { dirname, join } from "node:path"
import { daemonIpcRoutes } from "@goddard-ai/daemon-client/daemon-ipc"
import { createNodeClient } from "@goddard-ai/ipc/node"
import { readDaemonTcpAddressFromDaemonUrl } from "@goddard-ai/schema/daemon-url"
import { afterEach, expect, test } from "bun:test"

import type { BackendClient } from "../src/backend.ts"
import { startDaemonServer } from "../src/ipc.ts"
import { resetComposedDaemonStore, type ComposedDaemonStore } from "./support/store.ts"
import { removeTemporaryPath } from "./support/temp.ts"

const cleanup: Array<() => Promise<void>> = []
const originalHome = process.env.HOME
const rootConfigSchemaUrl =
  "https://raw.githubusercontent.com/goddard-ai/core/refs/heads/main/schema/json/goddard.json"
let db: ComposedDaemonStore = resetComposedDaemonStore({ filename: ":memory:" })

afterEach(async () => {
  while (cleanup.length > 0) {
    await cleanup.pop()?.()
  }

  db = resetComposedDaemonStore({ filename: ":memory:" })

  if (originalHome === undefined) {
    delete process.env.HOME
  } else {
    process.env.HOME = originalHome
  }
})

test("daemon IPC discovers and initializes workforce config through daemon-owned handlers", async () => {
  await useTempHome()
  await writeGlobalRootConfig({
    agents: {
      default: "configured-workforce-agent",
    },
  })
  const repoDir = await mkdtemp(join(tmpdir(), "goddard-workforce-init-"))
  const packageDir = join(repoDir, "packages", "ui")
  cleanup.push(() => removeTemporaryPath(repoDir))

  await mkdir(packageDir, { recursive: true })
  await writeFile(
    join(repoDir, "package.json"),
    JSON.stringify({ name: "@repo/root", private: true }, null, 2),
    "utf-8",
  )
  await writeFile(
    join(packageDir, "package.json"),
    JSON.stringify({ name: "@repo/ui", private: true }, null, 2),
    "utf-8",
  )
  expect(spawnSync("git", ["init"], { cwd: repoDir }).status).toBe(0)

  const daemon = await startDaemonServer(createTestBackendClient(), {
    port: 0,
    store: db,
  })
  cleanup.push(async () => {
    await daemon.close()
  })

  const client = createDaemonClient(daemon.daemonUrl)
  const discovered = await client.workforce.discoverCandidates({
    rootDir: packageDir,
  })
  const normalizedRootDir = await realpath(repoDir)

  expect(discovered.rootDir).toBe(normalizedRootDir)
  expect(discovered.candidates.map((candidate: any) => candidate.relativeDir)).toEqual([
    ".",
    "packages/ui",
  ])

  const initialized = await client.workforce.initialize({
    rootDir: packageDir,
    packageDirs: discovered.candidates.map((candidate: any) => candidate.rootDir),
  })

  const config = JSON.parse(await readFile(initialized.initialized.configPath, "utf-8")) as {
    defaultAgent: string
    rootAgentId: string
    agents: Array<{ id: string; cwd: string }>
  }

  expect(initialized.initialized.rootDir).toBe(normalizedRootDir)
  expect(config.defaultAgent).toBe("configured-workforce-agent")
  expect(config.rootAgentId).toBe("root")
  expect(config.agents.map((agent) => agent.cwd)).toEqual([".", "packages/ui"])
  await expect(readFile(initialized.initialized.ledgerPath, "utf-8")).resolves.toBe("")
})

test("daemon workforce event stream rejects inactive repositories", async () => {
  const rootDir = await mkdtemp(join(tmpdir(), "goddard-workforce-stream-"))
  cleanup.push(() => removeTemporaryPath(rootDir))

  const daemon = await startDaemonServer(createTestBackendClient(), {
    port: 0,
    store: db,
  })
  cleanup.push(async () => {
    await daemon.close()
  })

  const client = createDaemonClient(daemon.daemonUrl)
  const normalizedRootDir = await realpath(rootDir)

  await expect(client.workforce.streamEvents({ rootDir })).rejects.toThrow(
    `No workforce is running for ${normalizedRootDir}`,
  )
})

function createDaemonClient(daemonUrl: string) {
  return createNodeClient(
    readDaemonTcpAddressFromDaemonUrl(daemonUrl),
    daemonIpcRoutes as any,
  ) as any
}

async function useTempHome() {
  const homeDir = await mkdtemp(join(tmpdir(), "goddard-workforce-home-"))
  process.env.HOME = homeDir
  cleanup.push(() => removeTemporaryPath(homeDir))
}

async function writeGlobalRootConfig(config: Record<string, unknown>) {
  const configPath = join(process.env.HOME!, ".goddard", "config.json")
  await mkdir(dirname(configPath), { recursive: true })
  await writeFile(
    configPath,
    `${JSON.stringify({ $schema: rootConfigSchemaUrl, ...config }, null, 2)}\n`,
    "utf-8",
  )
}

function createTestBackendClient(): BackendClient {
  return {
    auth: {
      device: {
        start: async () => ({
          deviceCode: "dev_1",
          userCode: "ABCD-1234",
          verificationUri: "https://github.com/login/device",
          expiresIn: 900,
          interval: 5,
        }),
        complete: async () => ({
          token: "tok_1",
          githubUsername: "alec",
          githubUserId: 42,
        }),
      },
      session: {
        current: async () => ({
          token: "tok_1",
          githubUsername: "alec",
          githubUserId: 42,
        }),
      },
    },
    pullRequests: {
      create: async () => ({ number: 1, url: "https://example.com/pr/1" }),
      managed: async () => ({ managed: true }),
      comments: {
        create: async () => ({ success: true }),
      },
    },
    webhooks: {
      github: async () => ({ type: "noop" }),
    },
    remoteRepo: {
      stream: async () => new Response(),
    },
    stream: {
      subscribe: async () => {
        throw new Error("not used")
      },
    },
  } as unknown as BackendClient
}
