import { mkdir, mkdtemp, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { dirname, join } from "node:path"
import { createDaemonIpcClient } from "@goddard-ai/daemon-client/node"
import { getDatabasePath } from "@goddard-ai/paths/node"
import { afterEach, expect, test } from "bun:test"

import type { BackendClient } from "../src/backend.ts"
import { startDaemonServer } from "../src/ipc/server.ts"
import { main } from "../src/main.ts"
import { openComposedDaemonStore } from "../src/plugins.ts"
import { seedMockData } from "../src/seed/mock.ts"
import { send } from "./ipc-client-helpers.ts"
import { removeTemporaryPath } from "./support/temp.ts"

const cleanup: Array<() => Promise<void>> = []
const originalHome = process.env.HOME
const originalDataProfile = process.env.GODDARD_DATA_PROFILE

afterEach(async () => {
  restoreEnv("HOME", originalHome)
  restoreEnv("GODDARD_DATA_PROFILE", originalDataProfile)

  while (cleanup.length > 0) {
    await cleanup.pop()?.()
  }
})

test("seed mock writes deterministic isolated fixture data through the daemon IPC surface", async () => {
  await useTempHome()

  const seeded = await seedMockData({ reset: true })
  expect(seeded.databasePath).toBe(join(process.env.HOME!, ".goddard", "mock", "goddard.db"))
  expect(process.env.GODDARD_DATA_PROFILE).toBe(originalDataProfile)

  process.env.GODDARD_DATA_PROFILE = "mock"
  const daemon = await startDaemonServer(createTestBackendClient(), { port: 0 })
  cleanup.push(async () => {
    await daemon.close().catch(() => {})
  })
  const client = createDaemonIpcClient({ daemonUrl: daemon.daemonUrl })

  const sessions = await send(client, "session.list", { limit: 10 })
  expect(sessions.sessions).toHaveLength(8)
  expect(sessions.sessions.every((session: any) => !session.activeDaemonSession)).toBe(true)
  expect(sessions.sessions.every((session: any) => session.connectionMode !== "live")).toBe(true)
  expect(new Set(sessions.sessions.map((session: any) => session.status))).toEqual(
    new Set(["archived", "blocked", "cancelled", "done", "error", "idle"]),
  )
  expect(sessions.sessions.some((session: any) => session.repository === null)).toBe(true)
  expect(sessions.sessions.some((session: any) => session.contextUsage?.used > 190_000)).toBe(true)

  await expect(send(client, "session.connect", { id: "ses_mock_review_boundary" })).rejects.toThrow(
    /archived/i,
  )

  const history = await send(client, "session.history", { id: "ses_mock_review_boundary" })
  expect(history.connection).toEqual({
    mode: "history",
    reconnectable: false,
    activeDaemonSession: false,
  })
  expect(history.turns).toHaveLength(2)

  const contextLimit = await send(client, "session.get", { id: "ses_mock_context_limit" })
  expect(contextLimit.session.models?.availableModels).toHaveLength(2)
  expect(contextLimit.session.configOptions).toHaveLength(1)

  const inbox = await send(client, "inbox.list", {
    statuses: ["unread", "read", "saved", "archived", "replied", "completed"],
    limit: 10,
  })
  expect(inbox.items).toHaveLength(10)
  expect(new Set(inbox.items.map((item: any) => item.status))).toEqual(
    new Set(["unread", "read", "saved", "archived", "replied", "completed"]),
  )
  expect(
    inbox.items
      .filter((item: any) => item.entityId.startsWith("pr_"))
      .map((item: any) => item.reason),
  ).toEqual(expect.arrayContaining(["pull_request.created", "pull_request.updated"]))

  const pullRequest = await send(client, "pr.get", { id: "pr_mock_123" })
  expect(pullRequest.pullRequest).toMatchObject({
    host: "github",
    owner: "goddard-ai",
    repo: "goddard-ai",
    prNumber: 123,
  })
  const docsPullRequest = await send(client, "pr.get", { id: "pr_mock_docs_7" })
  expect(docsPullRequest.pullRequest).toMatchObject({
    host: "github",
    owner: "goddard-ai",
    repo: "docs",
    prNumber: 7,
  })
})

test("seed mock reset is mock-profile only and repeated seeding does not duplicate records", async () => {
  await useTempHome()
  process.env.GODDARD_DATA_PROFILE = "development"
  const developmentDatabasePath = getDatabasePath()
  await mkdir(dirname(developmentDatabasePath), { recursive: true })
  await writeFile(developmentDatabasePath, "development data")

  await main(["seed", "mock", "--reset"])
  await main(["seed", "mock"])

  process.env.GODDARD_DATA_PROFILE = "mock"
  const store = openComposedDaemonStore()
  try {
    expect(store.sessions.findMany()).toHaveLength(8)
    expect(store.sessionTurns.findMany()).toHaveLength(7)
    expect(store.inboxItems.findMany()).toHaveLength(10)
    const pullRequests = store.pullRequests.findMany()
    expect(pullRequests).toHaveLength(5)
    expect(new Set(pullRequests.map((pullRequest) => pullRequest.repo))).toEqual(
      new Set(["developer-tools", "docs", "goddard-ai"]),
    )
  } finally {
    store.close()
  }

  process.env.GODDARD_DATA_PROFILE = "development"
  expect(await Bun.file(developmentDatabasePath).text()).toBe("development data")
})

async function useTempHome() {
  const home = await mkdtemp(join(tmpdir(), "goddard-daemon-mock-home-"))
  process.env.HOME = home
  cleanup.push(async () => {
    await removeTemporaryPath(home)
  })
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
      create: async () => ({
        number: 1,
        url: "https://github.com/example/repo/pull/1",
      }),
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

function restoreEnv(key: string, value: string | undefined) {
  if (value === undefined) {
    delete process.env[key]
    return
  }

  process.env[key] = value
}
