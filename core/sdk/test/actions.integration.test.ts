import { startDaemonServer, type DaemonServer } from "@goddard-ai/daemon/ipc"
import { execFile } from "node:child_process"
import * as fs from "node:fs/promises"
import { createRequire } from "node:module"
import * as os from "node:os"
import * as path from "node:path"
import { promisify } from "node:util"
import { afterEach, expect, test, vi } from "vitest"
import { runAgent } from "../src/daemon/session/client.ts"

vi.mock("@goddard-ai/config", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@goddard-ai/config")>()
  return {
    ...actual,
    resolveDefaultAgent: vi.fn().mockResolvedValue("pi-acp"),
  }
})

import { buildActionSessionParams, resolveAction } from "../src/node/actions.ts"

const execFileAsync = promisify(execFile)

const { permissionsBySessionId, permissionsByToken, sessionStates, sessions } = vi.hoisted(() => ({
  sessions: new Map<string, unknown>(),
  sessionStates: new Map<string, any>(),
  permissionsBySessionId: new Map<string, any>(),
  permissionsByToken: new Map<string, any>(),
}))

vi.mock(
  "@goddard-ai/storage",
  async (importOriginal): Promise<typeof import("@goddard-ai/storage")> => {
    const actual = await importOriginal<typeof import("@goddard-ai/storage")>()

    return {
      ...actual,
      SessionStorage: {
        ...actual.SessionStorage,
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
        list: vi.fn(async () => Array.from(sessions.values())),
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
      SessionStateStorage: {
        ...actual.SessionStateStorage,
        create: vi.fn(async (record: any) => {
          const now = new Date().toISOString()
          const created = { ...record, createdAt: now, updatedAt: now }
          sessionStates.set(record.sessionId, created)
          return created
        }),
        list: vi.fn(async () => Array.from(sessionStates.values())),
        get: vi.fn(async (sessionId: string) => sessionStates.get(sessionId) ?? null),
        update: vi.fn(async (sessionId: string, data: any) => {
          const existing = sessionStates.get(sessionId)
          if (!existing) {
            return null
          }
          const updated = { ...existing, ...data, updatedAt: new Date().toISOString() }
          sessionStates.set(sessionId, updated)
          return updated
        }),
        appendHistory: vi.fn(async (sessionId: string, message: any) => {
          const existing = sessionStates.get(sessionId)
          if (!existing) {
            return null
          }
          const updated = {
            ...existing,
            history: [...existing.history, message],
            updatedAt: new Date().toISOString(),
          }
          sessionStates.set(sessionId, updated)
          return updated
        }),
        appendDiagnostic: vi.fn(async (sessionId: string, event: any) => {
          const existing = sessionStates.get(sessionId)
          if (!existing) {
            return null
          }
          const updated = {
            ...existing,
            diagnostics: [...existing.diagnostics, event],
            updatedAt: new Date().toISOString(),
          }
          sessionStates.set(sessionId, updated)
          return updated
        }),
        remove: vi.fn(async (sessionId: string) => {
          sessionStates.delete(sessionId)
        }),
      },
    }
  },
)

vi.mock(
  "@goddard-ai/storage/session-permissions",
  async (importOriginal): Promise<typeof import("@goddard-ai/storage/session-permissions")> => {
    const actual = await importOriginal<typeof import("@goddard-ai/storage/session-permissions")>()

    return {
      ...actual,
      SessionPermissionsStorage: {
        ...actual.SessionPermissionsStorage,
        create: vi.fn(async (record: any) => {
          const created = { ...record, createdAt: new Date().toISOString() }
          permissionsBySessionId.set(record.sessionId, created)
          permissionsByToken.set(record.token, created)
          return created
        }),
        get: vi.fn(async (sessionId: string) => permissionsBySessionId.get(sessionId) ?? null),
        getByToken: vi.fn(async (token: string) => permissionsByToken.get(token) ?? null),
        list: vi.fn(async () => Array.from(permissionsBySessionId.values())),
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
    }
  },
)

const cleanup: Array<() => Promise<void>> = []

afterEach(async () => {
  sessions.clear()
  sessionStates.clear()
  permissionsBySessionId.clear()
  permissionsByToken.clear()

  while (cleanup.length > 0) {
    await cleanup.pop()?.()
  }
})

test("resolved actions can run through the daemon-backed session client", async () => {
  process.env.HOME = await fs.mkdtemp(path.join(os.tmpdir(), "goddard-action-home-"))
  const tempDir = await fs.mkdtemp(path.join(os.tmpdir(), "goddard-action-daemon-"))
  const actionsDir = path.join(tempDir, ".goddard", "actions")
  const daemon = await startTestDaemon()
  const require = createRequire(import.meta.url)
  const exampleAgentPath = require.resolve("@agentclientprotocol/sdk/dist/examples/agent.js")

  cleanup.push(async () => {
    await fs.rm(tempDir, { recursive: true, force: true })
  })

  await fs.mkdir(actionsDir, { recursive: true })
  await initializeGitRepo(tempDir)
  await fs.writeFile(
    path.join(actionsDir, "review.md"),
    "Review the current diff carefully.\n",
    "utf-8",
  )
  await fs.writeFile(
    path.join(tempDir, ".goddard", "config.json"),
    JSON.stringify({
      actions: {
        session: {
          env: {
            ACTION_ROOT: "true",
          },
        },
      },
    }),
    "utf-8",
  )

  const action = await resolveAction("review", tempDir)
  const result = await runAgent(
    buildActionSessionParams(action, {
      cwd: tempDir,
      agent: {
        id: "node-agent",
        name: "Node Agent",
        version: "1.0.0",
        description: "Local node-based ACP test agent.",
        distribution: {
          binary: {
            "darwin-aarch64": {
              archive: "https://example.com/node-agent-darwin-aarch64.tar.gz",
              cmd: "node",
              args: [exampleAgentPath],
            },
            "darwin-x86_64": {
              archive: "https://example.com/node-agent-darwin-x86_64.tar.gz",
              cmd: "node",
              args: [exampleAgentPath],
            },
            "linux-aarch64": {
              archive: "https://example.com/node-agent-linux-aarch64.tar.gz",
              cmd: "node",
              args: [exampleAgentPath],
            },
            "linux-x86_64": {
              archive: "https://example.com/node-agent-linux-x86_64.tar.gz",
              cmd: "node",
              args: [exampleAgentPath],
            },
            "windows-aarch64": {
              archive: "https://example.com/node-agent-windows-aarch64.zip",
              cmd: "node",
              args: [exampleAgentPath],
            },
            "windows-x86_64": {
              archive: "https://example.com/node-agent-windows-x86_64.zip",
              cmd: "node",
              args: [exampleAgentPath],
            },
          },
        },
      },
      mcpServers: [],
      systemPrompt: "Keep responses short.",
    }),
    undefined,
    { daemonUrl: daemon.daemonUrl },
  )

  expect(result).toBeNull()
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

async function initializeGitRepo(cwd: string): Promise<void> {
  await execFileAsync("git", ["init"], { cwd })
  await execFileAsync("git", ["config", "user.name", "Goddard Test"], { cwd })
  await execFileAsync("git", ["config", "user.email", "test@example.com"], { cwd })
}
