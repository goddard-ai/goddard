import * as assert from "node:assert/strict"
import { mkdtemp, rm, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { afterEach, test, vi } from "vitest"
import { runAgent } from "../../src/daemon/session/client.ts"
import { startDaemonServer, type DaemonServer } from "../../../../daemon/src/ipc.ts"

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

test("sdk daemon session client runs against a real daemon server", async () => {
  const daemon = await startTestDaemon()
  const agentPath = await createPromptAgent()

  const created = await runAgent(
    {
      agent: {
        type: "binary",
        cmd: "node",
        args: [agentPath],
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
