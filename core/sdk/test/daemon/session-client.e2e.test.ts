import { startDaemonServer, type DaemonServer } from "@goddard-ai/daemon/ipc"
import { mkdtemp, rm, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { afterEach, assert, test, vi } from "vitest"
import { runAgent } from "../../src/daemon/session/client.js"

const { permissionsBySessionId, permissionsByToken, sessionStates, sessions } = vi.hoisted(() => ({
  sessions: new Map<string, unknown>(),
  sessionStates: new Map<string, any>(),
  permissionsBySessionId: new Map<string, any>(),
  permissionsByToken: new Map<string, any>(),
}))

vi.mock("@goddard-ai/storage", (): typeof import("@goddard-ai/storage") => ({
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
    list: vi.fn(async () => Array.from(sessions.values())),
    listRecent: vi.fn(
      async ({ limit, cursor }: { limit: number; cursor?: { updatedAt: Date; id: string } }) => {
        return Array.from(sessions.values())
          .sort((left: any, right: any) => {
            const updatedAtDiff = right.updatedAt.valueOf() - left.updatedAt.valueOf()
            if (updatedAtDiff !== 0) {
              return updatedAtDiff
            }

            return right.id.localeCompare(left.id)
          })
          .filter((record: any) => {
            if (!cursor) {
              return true
            }

            return (
              record.updatedAt.valueOf() < cursor.updatedAt.valueOf() ||
              (record.updatedAt.valueOf() === cursor.updatedAt.valueOf() && record.id < cursor.id)
            )
          })
          .slice(0, limit)
      },
    ),
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
  } as any
} as any))

vi.mock("@goddard-ai/storage/session-permissions", (): typeof import("@goddard-ai/storage/session-permissions") => ({
  SessionPermissionsStorage: {
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
  } as any
} as any))

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

test("sdk daemon session client runs against a real daemon server", async () => {
  const daemon = await startTestDaemon()
  const agentPath = await createPromptAgent()

  const created = await runAgent(
    {
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
              args: [agentPath],
            },
            "darwin-x86_64": {
              archive: "https://example.com/node-agent-darwin-x86_64.tar.gz",
              cmd: "node",
              args: [agentPath],
            },
            "linux-aarch64": {
              archive: "https://example.com/node-agent-linux-aarch64.tar.gz",
              cmd: "node",
              args: [agentPath],
            },
            "linux-x86_64": {
              archive: "https://example.com/node-agent-linux-x86_64.tar.gz",
              cmd: "node",
              args: [agentPath],
            },
            "windows-aarch64": {
              archive: "https://example.com/node-agent-windows-aarch64.zip",
              cmd: "node",
              args: [agentPath],
            },
            "windows-x86_64": {
              archive: "https://example.com/node-agent-windows-x86_64.zip",
              cmd: "node",
              args: [agentPath],
            },
          },
        },
      },
      cwd: process.cwd(),
      mcpServers: [],
      systemPrompt: "Keep responses short.",
    },
    undefined,
    { daemonUrl: daemon.daemonUrl },
  )

  assert.ok(created)

  const firstPrompt = await created.prompt("Say hello in one sentence.")
  assert.ok(firstPrompt)

  const firstHistory = await created.getHistory()
  assert.equal(firstHistory.length > 0, true)

  await created.stop()
}, 15_000)

async function startTestDaemon(): Promise<DaemonServer> {
  const socketDir = await mkdtemp(join(tmpdir(), "goddard-sdk-daemon-session-"))
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

async function createPromptAgent(): Promise<string> {
  const agentDir = await mkdtemp(join(tmpdir(), "goddard-sdk-agent-"))
  const agentPath = join(agentDir, "agent.mjs")

  cleanup.push(async () => {
    await rm(agentDir, { recursive: true, force: true })
  })

  await writeFile(
    agentPath,
    `#!/usr/bin/env node
import * as readline from "node:readline"

const rl = readline.createInterface({ input: process.stdin })

function send(message) {
  process.stdout.write(\`\${JSON.stringify(message)}\\n\`)
}

rl.on("line", (line) => {
  const message = JSON.parse(line)

  if (message.method === "initialize") {
    send({
      jsonrpc: "2.0",
      id: message.id,
      result: {
        protocolVersion: 1,
        agentCapabilities: { loadSession: false },
      },
    })
    return
  }

  if (message.method === "session/new") {
    send({
      jsonrpc: "2.0",
      id: message.id,
      result: { sessionId: "agent-session-1" },
    })
    return
  }

  if (message.method === "session/prompt") {
    send({
      jsonrpc: "2.0",
      method: "session/update",
      params: {
        sessionId: message.params.sessionId,
        update: {
          sessionUpdate: "agent_message_chunk",
          content: {
            type: "text",
            text: "Hello from the SDK daemon session test.",
          },
        },
      },
    })

    send({
      jsonrpc: "2.0",
      id: message.id,
      result: { stopReason: "end_turn" },
    })
    return
  }

  if (message.method === "session/cancel") {
    return
  }
})
`,
    "utf-8",
  )

  return agentPath
}
