import * as assert from "node:assert/strict"
import { mkdtemp, rm, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join } from "node:path"
import type { GetDaemonSessionHistoryResponse } from "@goddard-ai/schema/daemon"
import { daemonIpcSchema } from "@goddard-ai/schema/daemon-ipc"
import { afterEach, test, vi } from "vitest"
import { createNodeClient } from "../../core/ipc/src/index.ts"
import { createServer } from "../../core/ipc/src/server.ts"
import { createDaemonUrl } from "../../core/schema/src/daemon-url.ts"

const { permissionsBySessionId, permissionsByToken, sessionStates, sessions } = vi.hoisted(() => ({
  sessions: new Map<string, unknown>(),
  sessionStates: new Map<string, any>(),
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
}))

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
  const clients = new Map<
    string,
    ReturnType<typeof createNodeClient<typeof daemonIpcSchema>>
  >()
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

  const { createAppDaemonIpcClient } = await import("./daemon-session.ts")
  const daemon = await startTestDaemon()
  const client = createAppDaemonIpcClient(daemon.daemonUrl)

  const created = await client.send("sessionCreate", {
    agent: {
      type: "binary",
      cmd: "node",
      args: [await createPromptAgent()],
    },
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
  let nextSessionId = 0
  const sessionHistory = new Map<
    string,
    { acpId: string; history: GetDaemonSessionHistoryResponse["history"] }
  >()

  function getSessionResponse(id: string) {
    const session = sessionHistory.get(id)
    if (!session) {
      throw new Error("Session not found")
    }

    return {
      session: {
        id,
        acpId: session.acpId,
        status: "active" as const,
        agentName: "test-agent",
        cwd: process.cwd(),
        metadata: {},
        connection: {
          mode: "live" as const,
          reconnectable: true,
          historyAvailable: session.history.length > 0,
          activeDaemonSession: true,
        },
        diagnostics: {
          eventCount: 0,
          historyLength: session.history.length,
          lastEventAt: null,
        },
        createdAt: new Date().toISOString(),
        updatedAt: new Date().toISOString(),
        errorMessage: null,
        blockedReason: null,
        initiative: null,
        lastAgentMessage: null,
      },
    }
  }

  const ipcServer = createServer(socketPath, daemonIpcSchema, {
    health: async () => ({ ok: true }),
    prSubmit: async () => ({ number: 1, url: "https://github.com/example/repo/pull/1" }),
    prReply: async () => ({ success: true }),
    sessionCreate: async () => {
      const id = `daemon-session-${nextSessionId++}`
      const acpId = `acp-session-${nextSessionId}`
      sessionHistory.set(id, { acpId, history: [] })
      return getSessionResponse(id)
    },
    sessionGet: async ({ id }) => getSessionResponse(id),
    sessionConnect: async ({ id }) => getSessionResponse(id),
    sessionHistory: async ({ id }) => {
      const session = sessionHistory.get(id)
      if (!session) {
        throw new Error("Session not found")
      }
      return {
        id,
        acpId: session.acpId,
        connection: {
          mode: "live" as const,
          reconnectable: true,
          historyAvailable: session.history.length > 0,
          activeDaemonSession: true,
        },
        history: [...session.history],
      }
    },
    sessionDiagnostics: async ({ id }) => {
      const session = sessionHistory.get(id)
      if (!session) {
        throw new Error("Session not found")
      }
      return {
        id,
        acpId: session.acpId,
        connection: {
          mode: "live" as const,
          reconnectable: true,
          historyAvailable: session.history.length > 0,
          activeDaemonSession: true,
        },
        events: [],
      }
    },
    sessionShutdown: async ({ id }) => {
      return {
        id,
        success: sessionHistory.delete(id),
      }
    },
    sessionSend: async ({ id, message }) => {
      const session = sessionHistory.get(id)
      if (!session) {
        throw new Error("Session not found")
      }
      session.history.push(message as GetDaemonSessionHistoryResponse["history"][number])
      if (
        typeof message === "object" &&
        message !== null &&
        "id" in message &&
        (typeof message.id === "string" || typeof message.id === "number")
      ) {
        session.history.push({
          jsonrpc: "2.0",
          id: message.id,
          result: { stopReason: "end_turn" },
        })
      }
      return { accepted: true as const }
    },
    sessionResolveToken: async () => ({ id: "daemon-session-0" }),
  })

  cleanup.push(async () => {
    await new Promise<void>((resolve, reject) => {
      ipcServer.server.close((error) => {
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
        ipcServer.server.close((error) => {
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
