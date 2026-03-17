import { afterEach, test, vi } from "vitest"
import * as assert from "node:assert/strict"
import { mkdtemp, rm } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { createRequire } from "node:module"

const { permissionsBySessionId, permissionsByToken, sessions } = vi.hoisted(() => ({
  sessions: new Map<string, any>(),
  permissionsBySessionId: new Map<string, any>(),
  permissionsByToken: new Map<string, any>(),
}))

vi.mock("@goddard-ai/storage", () => ({
  SessionStorage: {
    create: vi.fn(async (record: any) => {
      const now = new Date()
      sessions.set(record.id, {
        ...record,
        createdAt: now,
        updatedAt: now,
        errorMessage: null,
        blockedReason: null,
        initiative: null,
        lastAgentMessage: null,
      })
    }),
    get: vi.fn(async (id: string) => sessions.get(id) ?? null),
    update: vi.fn(async (id: string, data: any) => {
      const existing = sessions.get(id)
      if (!existing) {
        return
      }
      sessions.set(id, {
        ...existing,
        ...data,
        updatedAt: new Date(),
      })
    }),
  },
}))

vi.mock("@goddard-ai/storage/session-permissions", () => ({
  SessionPermissionsStorage: {
    create: vi.fn(async (record: any) => {
      const created = { ...record, createdAt: new Date().toISOString() }
      permissionsBySessionId.set(record.sessionId, created)
      permissionsByToken.set(record.token, created)
      return created
    }),
    get: vi.fn(async (sessionId: string) => permissionsBySessionId.get(sessionId) ?? null),
    getByToken: vi.fn(async (token: string) => permissionsByToken.get(token) ?? null),
    addAllowedPr: vi.fn(async (sessionId: string, prNumber: number) => {
      const existing = permissionsBySessionId.get(sessionId)
      if (!existing) {
        return
      }
      if (!existing.allowedPrNumbers.includes(prNumber)) {
        existing.allowedPrNumbers = [...existing.allowedPrNumbers, prNumber]
      }
    }),
    revoke: vi.fn(async (sessionId: string) => {
      const existing = permissionsBySessionId.get(sessionId)
      if (!existing) {
        return
      }
      permissionsBySessionId.delete(sessionId)
      permissionsByToken.delete(existing.token)
    }),
  },
}))

import { SessionPermissionsStorage } from "@goddard-ai/storage/session-permissions"
import { createDaemonIpcClient } from "../src/client.ts"
import { startDaemonServer, type DaemonServer } from "../src/ipc.ts"

const cleanup: Array<() => Promise<void>> = []

afterEach(async () => {
  sessions.clear()
  permissionsBySessionId.clear()
  permissionsByToken.clear()

  while (cleanup.length > 0) {
    await cleanup.pop()?.()
  }
})

test("daemon revokes session tokens when agent processes exit", async () => {
  const daemon = await startTestDaemon()
  const client = createDaemonIpcClient({ daemonUrl: daemon.daemonUrl })

  const require = createRequire(import.meta.url)
  const exampleAgentPath = require.resolve("@agentclientprotocol/sdk/dist/examples/agent.js")

  const created = await client.send("sessionCreate", {
    agent: {
      type: "binary",
      cmd: "node",
      args: [exampleAgentPath],
    },
    cwd: process.cwd(),
    mcpServers: [],
    systemPrompt: "Keep responses short.",
  })

  const permissions = await SessionPermissionsStorage.get(created.session.id)
  assert.ok(permissions)
  assert.equal(typeof permissions.token, "string")

  await client.send("sessionShutdown", { id: created.session.id })

  await waitFor(async () => {
    return (await SessionPermissionsStorage.getByToken(permissions.token)) === null
  })

  assert.equal(await SessionPermissionsStorage.getByToken(permissions.token), null)
})

async function startTestDaemon(): Promise<DaemonServer> {
  const socketDir = await mkdtemp(join(tmpdir(), "goddard-daemon-session-"))
  const socketPath = join(socketDir, "daemon.sock")

  const daemon = await startDaemonServer(
    {
      pr: {
        create: async () => ({
          number: 1,
          url: "https://github.com/example/repo/pull/1",
        }),
        reply: async () => ({ success: true }),
      },
    },
    { socketPath },
  )

  cleanup.push(async () => {
    await daemon.close().catch(() => {})
    await rm(socketDir, { recursive: true, force: true })
  })

  return daemon
}

async function waitFor(check: () => Promise<boolean>, timeoutMs = 3_000) {
  const start = Date.now()
  while (Date.now() - start < timeoutMs) {
    if (await check()) {
      return
    }
    await new Promise((resolve) => setTimeout(resolve, 25))
  }

  throw new Error("Timed out waiting for condition")
}
