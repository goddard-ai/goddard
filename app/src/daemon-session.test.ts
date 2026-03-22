import { createNodeClient } from "@goddard-ai/ipc"
import { createServer } from "@goddard-ai/ipc/server"
import { daemonIpcSchema } from "@goddard-ai/schema/daemon-ipc"
import { createDaemonUrl } from "@goddard-ai/schema/daemon-url"
import { mkdtemp, rm, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { afterEach, assert, test, vi } from "vitest"
import { createDaemonSessionTestIpcHandlers } from "./daemon-ipc-test-handlers.js"

const { permissionsBySessionId, permissionsByToken, sessionStates, sessions } = vi.hoisted(() => ({
  sessions: new Map<string, any>(),
  sessionStates: new Map<string, any>(),
  permissionsBySessionId: new Map<string, any>(),
  permissionsByToken: new Map<string, any>(),
}))

function createNodeAgent(agentPath: string) {
  return {
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
  }
}

vi.mock(
  "@goddard-ai/storage",
  async (importOriginal): Promise<typeof import("@goddard-ai/storage")> => ({
    ...(await importOriginal<typeof import("@goddard-ai/storage")>()),
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
      getByAcpId: vi.fn(
        async (acpId: string) =>
          Array.from(sessions.values()).find((s: any) => s.acpId === acpId) ?? null,
      ),
      listAll: vi.fn(async () => Array.from(sessions.values())),
      listRecent: vi.fn(async () => Array.from(sessions.values())),
      listByRepository: vi.fn(async (repository: string) =>
        Array.from(sessions.values()).filter((s: any) => s.repository === repository),
      ),
      listByRepositoryPr: vi.fn(async (repository: string, prNumber: number) =>
        Array.from(sessions.values()).filter(
          (s: any) => s.repository === repository && s.prNumber === prNumber,
        ),
      ),
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
    },
  }),
)

vi.mock(
  "@goddard-ai/storage/session-permissions",
  async (importOriginal): Promise<typeof import("@goddard-ai/storage/session-permissions")> => ({
    ...(await importOriginal<typeof import("@goddard-ai/storage/session-permissions")>()),
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
    },
  }),
)

const cleanup: Array<() => Promise<void>> = []

afterEach(async () => {
  vi.resetModules()
  vi.clearAllMocks()
  sessions.clear()
  sessionStates.clear()
  permissionsBySessionId.clear()
  permissionsByToken.clear()

  while (cleanup.length > 0) {
    await cleanup.pop()?.()
  }
})

test("app daemon session options create/connect/send/history/shutdown through the Tauri IPC bridge", async () => {
  const bridgeListeners = new Set<(event: { payload: unknown }) => void>()
  const subscriptions = new Map<string, () => Promise<void> | void>()
  const clients = new Map<string, ReturnType<typeof createNodeClient<typeof daemonIpcSchema>>>()
  let nextSubscriptionId = 0

  vi.doMock("@tauri-apps/api/event", () => ({
    listen: vi.fn(async (_eventName: string, handler: (event: { payload: unknown }) => void) => {
      bridgeListeners.add(handler)
      return () => {
        bridgeListeners.delete(handler)
      }
    }),
  }))

  vi.doMock("@tauri-apps/api/core", () => ({
    invoke: vi.fn(async (command: string, payload: Record<string, unknown>) => {
      const socketPath = payload.socketPath
      if (typeof socketPath !== "string") {
        throw new Error("socketPath must be a string")
      }

      let client = clients.get(socketPath)
      if (!client) {
        client = createNodeClient(socketPath, daemonIpcSchema)
        clients.set(socketPath, client)
      }

      if (command === "plugin:ipc|send") {
        return await client.send(payload.name as never, payload.payload as never)
      }

      if (command === "plugin:ipc|subscribe") {
        const subscriptionId = `tauri-sub-${nextSubscriptionId++}`
        const unsubscribe = await client.subscribe(payload.name as never, (streamPayload) => {
          for (const listener of bridgeListeners) {
            listener({
              payload: {
                subscriptionId,
                socketPath,
                name: payload.name,
                payload: streamPayload,
              },
            })
          }
        })

        subscriptions.set(subscriptionId, unsubscribe)
        return subscriptionId
      }

      if (command === "plugin:ipc|unsubscribe") {
        const subscriptionId = payload.subscriptionId
        if (typeof subscriptionId !== "string") {
          throw new Error("subscriptionId must be a string")
        }
        await Promise.resolve(subscriptions.get(subscriptionId)?.()).catch(() => {})
        subscriptions.delete(subscriptionId)
        return null
      }

      throw new Error(`Unexpected Tauri IPC command: ${command}`)
    }),
  }))

  const { createAppDaemonIpcClient } = await import("./daemon-session.js")
  const daemon = await startTestDaemon()
  const client = createAppDaemonIpcClient(daemon.daemonUrl)

  const created = await client.send("sessionCreate", {
    agent: createNodeAgent(await createPromptAgent()),
    cwd: process.cwd(),
    mcpServers: [],
    systemPrompt: "Keep responses short.",
  })

  const connected = await client.send("sessionConnect", { id: created.session.id })
  assert.equal(connected.session.id, created.session.id)

  await client.send("sessionSend", {
    id: created.session.id,
    message: {
      jsonrpc: "2.0",
      id: 1,
      method: "session/prompt",
      params: {
        sessionId: created.session.acpId,
        prompt: [{ type: "text", text: "Say hello in one sentence." }],
      },
    },
  })

  await waitFor(async () => {
    const history = await client.send("sessionHistory", { id: created.session.id })
    return history.history.some((message: unknown) => {
      return (
        typeof message === "object" &&
        message !== null &&
        "id" in message &&
        message.id === 1 &&
        "result" in message
      )
    })
  })

  const history = await client.send("sessionHistory", { id: created.session.id })
  assert.equal(history.history.length > 0, true)

  const shutdown = await client.send("sessionShutdown", { id: created.session.id })
  assert.equal(shutdown.success, true)
}, 15_000)

async function startTestDaemon(): Promise<{
  daemonUrl: string
  close: () => Promise<void>
}> {
  const socketDir = await mkdtemp(join(tmpdir(), "goddard-app-daemon-"))
  const socketPath = join(socketDir, "daemon.sock")
  const ipcServer = createServer(socketPath, daemonIpcSchema, createDaemonSessionTestIpcHandlers())

  cleanup.push(async () => {
    await new Promise<void>((resolve, reject) => {
      ipcServer.server.close((error: any) => {
        if (error) {
          reject(error)
          return
        }
        resolve()
      })
    }).catch(() => {})
    await rm(socketDir, { recursive: true, force: true })
  })

  return {
    daemonUrl: createDaemonUrl(socketPath),
    close: async () => {
      await new Promise<void>((resolve, reject) => {
        ipcServer.server.close((error: any) => {
          if (error) {
            reject(error)
            return
          }
          resolve()
        })
      })
    },
  }
}

async function createPromptAgent(): Promise<string> {
  const agentDir = await mkdtemp(join(tmpdir(), "goddard-app-agent-"))
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
            text: "Hello from the app transport test.",
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

async function waitFor(check: () => Promise<boolean>, timeoutMs = 5_000) {
  const start = Date.now()
  while (Date.now() - start < timeoutMs) {
    if (await check()) {
      return
    }
    await new Promise((resolve) => setTimeout(resolve, 25))
  }

  throw new Error("Timed out waiting for condition")
}
