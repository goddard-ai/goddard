import * as assert from "node:assert/strict"
import * as fs from "node:fs/promises"
import * as os from "node:os"
import * as path from "node:path"
import { createRequire } from "node:module"
import { afterEach, test, vi } from "vitest"
import { dedent } from "radashi"
import { buildActionSessionParams, resolveAction } from "../src/node/actions.ts"
import { runAgent } from "../src/daemon/session/client.ts"
import { startDaemonServer, type DaemonServer } from "../../../daemon/src/ipc.ts"

const { permissionsBySessionId, permissionsByToken, sessions } = vi.hoisted(() => ({
  sessions: new Map<string, unknown>(),
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
      if (!existing || typeof existing !== "object" || existing === null) {
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

const cleanup: Array<() => Promise<void>> = []

afterEach(async () => {
  sessions.clear()
  permissionsBySessionId.clear()
  permissionsByToken.clear()

  while (cleanup.length > 0) {
    await cleanup.pop()?.()
  }
})

test("resolved actions can run through the daemon-backed session client", async () => {
  const tempDir = await fs.mkdtemp(path.join(os.tmpdir(), "goddard-action-daemon-"))
  const actionsDir = path.join(tempDir, ".goddard", "actions")
  const daemon = await startTestDaemon()
  const require = createRequire(import.meta.url)
  const exampleAgentPath = require.resolve("@agentclientprotocol/sdk/dist/examples/agent.js")

  cleanup.push(async () => {
    await fs.rm(tempDir, { recursive: true, force: true })
  })

  await fs.mkdir(actionsDir, { recursive: true })
  await fs.writeFile(
    path.join(actionsDir, "review.md"),
    dedent`
      ---
      systemPrompt: "Start with the action checklist."
      ---
      Review the current diff carefully.
    `,
    "utf-8",
  )

  const action = await resolveAction("review", tempDir)
  const result = await runAgent(
    buildActionSessionParams(action, {
      cwd: tempDir,
      agent: {
        type: "binary",
        cmd: "node",
        args: [exampleAgentPath],
      },
      mcpServers: [],
      systemPrompt: "Keep responses short.",
    }),
    undefined,
    { daemonUrl: daemon.daemonUrl },
  )

  assert.equal(result, null)
})

async function startTestDaemon(): Promise<DaemonServer> {
  const socketDir = await fs.mkdtemp(path.join(os.tmpdir(), "goddard-action-session-"))
  const socketPath = path.join(socketDir, "daemon.sock")

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
    await fs.rm(socketDir, { recursive: true, force: true })
  })

  return daemon
}
