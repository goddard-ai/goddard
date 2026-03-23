import { mkdtemp, rm, writeFile } from "node:fs/promises"
import { createRequire } from "node:module"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { afterEach, expect, test, vi } from "vitest"
import { configureDaemonLogging } from "../src/logging.ts"

const {
  permissionsBySessionId,
  permissionsByToken,
  sessionStates,
  sessions,
  worktreeConstructorMock,
  worktreeSetupMock,
  worktreeCleanupMock,
} = vi.hoisted(() => ({
  sessions: new Map<string, any>(),
  sessionStates: new Map<string, any>(),
  permissionsBySessionId: new Map<string, any>(),
  permissionsByToken: new Map<string, any>(),
  worktreeConstructorMock: vi.fn(),
  worktreeSetupMock: vi.fn(),
  worktreeCleanupMock: vi.fn(() => true),
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
        listAll: vi.fn(async () => Array.from(sessions.values())),
        list: vi.fn(async () => Array.from(sessions.values())),
        listRecent: vi.fn(
          async ({
            limit,
            cursor,
          }: {
            limit: number
            cursor?: { updatedAt: Date; id: string }
          }) => {
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
                  (record.updatedAt.valueOf() === cursor.updatedAt.valueOf() &&
                    record.id < cursor.id)
                )
              })
              .slice(0, limit)
          },
        ),
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

vi.mock(
  "@goddard-ai/worktree",
  async (importOriginal): Promise<typeof import("@goddard-ai/worktree")> => {
    const actual = await importOriginal<typeof import("@goddard-ai/worktree")>()

    return {
      ...actual,
      Worktree: class {
        poweredBy = "mock-worktree"
        cwd: string

        constructor(options: { cwd: string }) {
          this.cwd = options.cwd
          worktreeConstructorMock(options)
        }

        setup(branchName: string) {
          worktreeSetupMock(branchName)
          return {
            worktreeDir: this.cwd,
            branchName,
          }
        }

        cleanup(worktreeDir: string, branchName: string) {
          return worktreeCleanupMock(worktreeDir, branchName)
        }
      },
    }
  },
)

import { createDaemonIpcClient } from "@goddard-ai/daemon-client"
import { SessionPermissionsStorage } from "@goddard-ai/storage/session-permissions"
import { startDaemonServer, type DaemonServer } from "../src/ipc.ts"

const cleanup: Array<() => Promise<void>> = []

afterEach(async () => {
  sessions.clear()
  sessionStates.clear()
  permissionsBySessionId.clear()
  permissionsByToken.clear()
  worktreeConstructorMock.mockClear()
  worktreeSetupMock.mockClear()
  worktreeCleanupMock.mockClear()

  while (cleanup.length > 0) {
    await cleanup.pop()?.()
  }
})

function createNodeAgent(agentPath: string) {
  const unixWrapper = `#!/bin/sh
exec "${process.execPath}" "${agentPath}" "$@"
`
  const windowsWrapper = `@echo off
"${process.execPath}" "${agentPath}" %*
`
  const unixArchive = `data:text/plain;base64,${Buffer.from(unixWrapper).toString("base64")}`
  const windowsArchive = `data:text/plain;base64,${Buffer.from(windowsWrapper).toString("base64")}`

  return {
    id: "node-agent",
    name: "Node Agent",
    version: "1.0.0",
    description: "Local node-based ACP test agent.",
    distribution: {
      binary: {
        "darwin-aarch64": {
          archive: unixArchive,
          cmd: "agent",
        },
        "darwin-x86_64": {
          archive: unixArchive,
          cmd: "agent",
        },
        "linux-aarch64": {
          archive: unixArchive,
          cmd: "agent",
        },
        "linux-x86_64": {
          archive: unixArchive,
          cmd: "agent",
        },
        "windows-aarch64": {
          archive: windowsArchive,
          cmd: "agent.cmd",
        },
        "windows-x86_64": {
          archive: windowsArchive,
          cmd: "agent.cmd",
        },
      },
    },
  }
}

test("daemon revokes session tokens when agent processes exit", async () => {
  const daemon = await startTestDaemon()
  const client = createDaemonIpcClient({ daemonUrl: daemon.daemonUrl })

  const require = createRequire(import.meta.url)
  const exampleAgentPath = require.resolve("@agentclientprotocol/sdk/dist/examples/agent.js")

  const created = await client.send("sessionCreate", {
    agent: createNodeAgent(exampleAgentPath),
    cwd: process.cwd(),
    mcpServers: [],
    systemPrompt: "Keep responses short.",
  })

  const permissions = await SessionPermissionsStorage.get(created.session.id)
  expect(permissions).toBeTruthy()
  expect(typeof permissions.token).toBe("string")

  await client.send("sessionShutdown", { id: created.session.id })

  await waitFor(async () => {
    return (await SessionPermissionsStorage.getByToken(permissions.token)) === null
  })

  expect(await SessionPermissionsStorage.getByToken(permissions.token)).toBeNull()
})

test("daemon persists repository context into direct session columns", async () => {
  const daemon = await startTestDaemon()
  const client = createDaemonIpcClient({ daemonUrl: daemon.daemonUrl })

  const require = createRequire(import.meta.url)
  const exampleAgentPath = require.resolve("@agentclientprotocol/sdk/dist/examples/agent.js")

  const created = await client.send("sessionCreate", {
    agent: createNodeAgent(exampleAgentPath),
    cwd: process.cwd(),
    mcpServers: [],
    systemPrompt: "Keep responses short.",
    repository: "acme/widgets",
    prNumber: 12,
    metadata: {
      workforce: { agentId: "reviewer", requestId: "req-1" },
    },
  })

  expect(created.session.repository).toBe("acme/widgets")
  expect(created.session.prNumber).toBe(12)
  expect(created.session.metadata).toEqual({
    workforce: { agentId: "reviewer", requestId: "req-1" },
  })
  expect(sessions.get(created.session.id)).toMatchObject({
    repository: "acme/widgets",
    prNumber: 12,
    metadata: {
      workforce: { agentId: "reviewer", requestId: "req-1" },
    },
  })

  await client.send("sessionShutdown", { id: created.session.id })
})

test("daemon reconciles interrupted sessions on restart and leaves archived history readable", async () => {
  const sessionId = "session-restart-1"
  sessions.set(sessionId, {
    id: sessionId,
    acpId: "acp-restart-1",
    status: "active",
    agentName: "node",
    cwd: process.cwd(),
    mcpServers: [],
    metadata: {
      worktree: {
        repoRoot: process.cwd(),
        requestedCwd: process.cwd(),
        effectiveCwd: "/tmp/mock-worktree/session-session-restart-1",
        worktreeDir: "/tmp/mock-worktree/session-session-restart-1",
        branchName: "session-session-restart-1",
        poweredBy: "mock-worktree",
      },
    },
    createdAt: new Date(),
    updatedAt: new Date(),
    errorMessage: null,
    blockedReason: null,
    initiative: null,
    lastAgentMessage: null,
  })
  sessionStates.set(sessionId, {
    sessionId,
    acpId: "acp-restart-1",
    connectionMode: "live",
    history: [{ jsonrpc: "2.0", method: "session/update", params: { value: "persisted" } }],
    diagnostics: [],
    activeDaemonSession: true,
    createdAt: new Date().toISOString(),
    updatedAt: new Date().toISOString(),
  })
  permissionsBySessionId.set(sessionId, {
    sessionId,
    token: "tok-restart-1",
    owner: "acme",
    repo: "widgets",
    allowedPrNumbers: [12],
    createdAt: new Date().toISOString(),
  })
  permissionsByToken.set("tok-restart-1", permissionsBySessionId.get(sessionId))

  const daemon = await startTestDaemon()
  const client = createDaemonIpcClient({ daemonUrl: daemon.daemonUrl })

  const session = await client.send("sessionGet", { id: sessionId })
  expect(session.session.status).toBe("error")
  expect(session.session.connection.mode).toBe("history")
  expect(session.session.connection.reconnectable).toBe(false)
  expect(session.session.errorMessage ?? "").toMatch(/previous daemon exited unexpectedly/i)

  const history = await client.send("sessionHistory", { id: sessionId })
  expect(history.connection.mode).toBe("history")
  expect(history.history).toHaveLength(1)

  const diagnostics = await client.send("sessionDiagnostics", { id: sessionId })
  expect(
    diagnostics.events.some((event) => event.type === "session_reconciled_after_restart"),
  ).toBe(true)
  expect(worktreeCleanupMock).toHaveBeenCalledWith(
    "/tmp/mock-worktree/session-session-restart-1",
    "session-session-restart-1",
  )
  await expect(client.send("sessionConnect", { id: sessionId })).rejects.toThrow(/archived/i)
  await expect(client.send("sessionResolveToken", { token: "tok-restart-1" })).rejects.toThrow(
    /invalid session token/i,
  )
})

test("multiple clients can observe the same live session stream independently", async () => {
  const daemon = await startTestDaemon()
  const clientA = createDaemonIpcClient({ daemonUrl: daemon.daemonUrl })
  const clientB = createDaemonIpcClient({ daemonUrl: daemon.daemonUrl })
  const require = createRequire(import.meta.url)
  const exampleAgentPath = require.resolve("@agentclientprotocol/sdk/dist/examples/agent.js")

  const created = await clientA.send("sessionCreate", {
    agent: createNodeAgent(exampleAgentPath),
    cwd: process.cwd(),
    mcpServers: [],
    systemPrompt: "Keep responses short.",
  })

  const clientAMessages: any[] = []
  const clientBMessages: any[] = []
  const unsubscribeA = await clientA.subscribe("sessionMessage", (payload) => {
    if (payload.id === created.session.id) {
      clientAMessages.push(payload.message)
    }
  })
  const unsubscribeB = await clientB.subscribe("sessionMessage", (payload) => {
    if (payload.id === created.session.id) {
      clientBMessages.push(payload.message)
    }
  })

  await clientA.send("sessionSend", {
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
    return clientAMessages.length > 0 && clientBMessages.length > 0
  })

  await Promise.resolve(unsubscribeA()).catch(() => {})
  await Promise.resolve(unsubscribeB()).catch(() => {})
  expect(clientAMessages.length > 0).toBe(true)
  expect(clientBMessages.length > 0).toBe(true)
})

test("daemon sessions keep the local cwd by default even inside git repositories", async () => {
  const daemon = await startTestDaemon()
  const client = createDaemonIpcClient({ daemonUrl: daemon.daemonUrl })
  const require = createRequire(import.meta.url)
  const exampleAgentPath = require.resolve("@agentclientprotocol/sdk/dist/examples/agent.js")
  const requestedCwd = join(process.cwd(), "src")
  const setupCallsBefore = worktreeSetupMock.mock.calls.length

  const created = await client.send("sessionCreate", {
    agent: createNodeAgent(exampleAgentPath),
    cwd: requestedCwd,
    mcpServers: [],
    systemPrompt: "Keep responses short.",
  })

  expect(created.session.cwd).toBe(join(process.cwd(), "src"))
  expect(created.session.metadata).toBeNull()
  expect(worktreeSetupMock.mock.calls.length).toBe(setupCallsBefore)
})

test("non-repository session cwd values skip worktree isolation", async () => {
  const daemon = await startTestDaemon()
  const client = createDaemonIpcClient({ daemonUrl: daemon.daemonUrl })
  const require = createRequire(import.meta.url)
  const exampleAgentPath = require.resolve("@agentclientprotocol/sdk/dist/examples/agent.js")
  const cwd = await mkdtemp(join(tmpdir(), "goddard-daemon-nonrepo-"))

  cleanup.push(async () => {
    await rm(cwd, { recursive: true, force: true })
  })

  const setupCallsBefore = worktreeSetupMock.mock.calls.length
  const created = await client.send("sessionCreate", {
    agent: createNodeAgent(exampleAgentPath),
    cwd,
    mcpServers: [],
    systemPrompt: "Keep responses short.",
  })

  expect(created.session.cwd).toBe(cwd)
  expect(created.session.metadata).toBeNull()
  expect(worktreeSetupMock.mock.calls.length).toBe(setupCallsBefore)
})

test("session worktree opt-in runs inside the mapped worktree subdirectory", async () => {
  const daemon = await startTestDaemon()
  const client = createDaemonIpcClient({ daemonUrl: daemon.daemonUrl })
  const require = createRequire(import.meta.url)
  const exampleAgentPath = require.resolve("@agentclientprotocol/sdk/dist/examples/agent.js")
  const requestedCwd = join(process.cwd(), "src")
  const setupCallsBefore = worktreeSetupMock.mock.calls.length

  const created = await client.send("sessionCreate", {
    agent: createNodeAgent(exampleAgentPath),
    cwd: requestedCwd,
    worktree: { enabled: true },
    mcpServers: [],
    systemPrompt: "Keep responses short.",
  })

  expect(created.session.cwd).toBe(requestedCwd)
  expect(created.session.metadata).toEqual(
    expect.objectContaining({
      worktree: expect.objectContaining({
        requestedCwd,
        effectiveCwd: requestedCwd,
        poweredBy: "mock-worktree",
      }),
    }),
  )
  expect(worktreeSetupMock.mock.calls.length).toBe(setupCallsBefore + 1)
})

test("one-shot daemon sessions clean up their worktree after the initial prompt completes when enabled", async () => {
  const daemon = await startTestDaemon()
  const client = createDaemonIpcClient({ daemonUrl: daemon.daemonUrl })
  const require = createRequire(import.meta.url)
  const exampleAgentPath = require.resolve("@agentclientprotocol/sdk/dist/examples/agent.js")

  const created = await client.send("sessionCreate", {
    agent: createNodeAgent(exampleAgentPath),
    cwd: process.cwd(),
    worktree: { enabled: true },
    mcpServers: [],
    systemPrompt: "Keep responses short.",
    oneShot: true,
    initialPrompt: "Say hello and stop.",
  })

  expect(created.session.status).toBe("done")

  await waitFor(async () => {
    return worktreeCleanupMock.mock.calls.length > 0
  })

  expect(worktreeCleanupMock).toHaveBeenCalled()
})

test("daemon logs agent message and chunk traffic without persisting high-volume events", async () => {
  const daemon = await startTestDaemon()
  const client = createDaemonIpcClient({ daemonUrl: daemon.daemonUrl })
  const longPrompt = "p".repeat(700)
  const longReply = "r".repeat(700)
  const agentPath = await createAgentScript(`
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
          result: { sessionId: "agent-session-logging" },
        })
        return
      }

      if (message.method === "session/prompt") {
        send({
          jsonrpc: "2.0",
          id: message.id,
          result: {
            stopReason: "end_turn",
            content: [{ type: "text", text: ${JSON.stringify(longReply)} }],
          },
        })
      }
    })
  `)

  const { logs, result } = await captureDaemonLogs(async () => {
    const created = await client.send("sessionCreate", {
      agent: createNodeAgent(agentPath),
      cwd: process.cwd(),
      mcpServers: [],
      systemPrompt: "Keep responses short.",
    })

    await client.send("sessionSend", {
      id: created.session.id,
      message: {
        jsonrpc: "2.0",
        id: 1,
        method: "session/prompt",
        params: {
          sessionId: created.session.acpId,
          prompt: [{ type: "text", text: longPrompt }],
        },
      },
    })

    await waitFor(async () => {
      const session = await client.send("sessionGet", { id: created.session.id })
      return session.session.status === "done"
    })

    return {
      sessionId: created.session.id,
      acpId: created.session.acpId,
    }
  })

  const writeLog = logs.find(
    (entry) =>
      entry.event === "agent.message_write" &&
      entry.sessionId === result.sessionId &&
      entry.method === "session/prompt",
  )
  expect(writeLog).toBeTruthy()
  expect(writeLog?.acpId).toBe(result.acpId)
  expect(writeLog?.hasId).toBe(true)
  const writeMessage = writeLog?.message as {
    params: {
      prompt: Array<{ text?: { text: string; byteLength: number; truncated: boolean } }>
    }
  }
  expect(writeMessage?.params.prompt[1]?.text).toEqual({
    text: `${longPrompt.slice(0, 512)}...`,
    byteLength: Buffer.byteLength(longPrompt),
    truncated: true,
  })

  const readLog = logs.find(
    (entry) => entry.event === "agent.message_read" && entry.sessionId === result.sessionId,
  )
  expect(readLog).toBeTruthy()
  expect(readLog?.acpId).toBe(result.acpId)
  expect(readLog?.hasId).toBe(true)
  const readMessage = readLog?.message as {
    result: {
      content: Array<{ text?: { text: string; byteLength: number; truncated: boolean } }>
    }
  }
  expect(readMessage?.result.content[0]?.text).toEqual({
    text: `${longReply.slice(0, 512)}...`,
    byteLength: Buffer.byteLength(longReply),
    truncated: true,
  })

  const chunkLog = logs.find(
    (entry) =>
      entry.event === "agent.chunk_read" &&
      entry.sessionId === result.sessionId &&
      (entry.preview as { truncated?: boolean } | undefined)?.truncated === true,
  )
  expect(chunkLog).toBeTruthy()
  expect(chunkLog?.acpId).toBe(result.acpId)
  const chunkPreview = chunkLog?.preview as
    | { text: string; truncated: boolean; byteLength: number }
    | undefined
  expect(typeof chunkPreview?.text).toBe("string")
  expect(chunkPreview?.truncated).toBe(true)
  expect((chunkPreview?.byteLength ?? 0) > 256).toBe(true)

  const statusChangeLog = logs.find(
    (entry) =>
      entry.event === "session_status_changed" &&
      entry.sessionId === result.sessionId &&
      entry.previousStatus === "active" &&
      entry.nextStatus === "done",
  )
  expect(statusChangeLog).toBeTruthy()
  expect(statusChangeLog?.reason).toBe("agent_message")

  const diagnostics = await client.send("sessionDiagnostics", { id: result.sessionId })
  expect(
    diagnostics.events.some(
      (event) => event.type === "agent.message_read" || event.type === "agent.chunk_read",
    ),
  ).toBe(false)
})

test("malformed runtime agent output is surfaced through diagnostics and archived state", async () => {
  const daemon = await startTestDaemon()
  const client = createDaemonIpcClient({ daemonUrl: daemon.daemonUrl })
  const agentPath = await createAgentScript(`
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
          result: { sessionId: "agent-session-malformed" },
        })
        return
      }

      if (message.method === "session/prompt") {
        process.stdout.write("not-json\\n")
        process.exit(9)
      }
    })
  `)

  const created = await client.send("sessionCreate", {
    agent: createNodeAgent(agentPath),
    cwd: process.cwd(),
    mcpServers: [],
    systemPrompt: "Keep responses short.",
  })

  await client.send("sessionSend", {
    id: created.session.id,
    message: {
      jsonrpc: "2.0",
      id: 1,
      method: "session/prompt",
      params: {
        sessionId: created.session.acpId,
        prompt: [{ type: "text", text: "Break the stream." }],
      },
    },
  })

  await waitFor(async () => {
    const session = await client.send("sessionGet", { id: created.session.id })
    return session.session.status === "error"
  })

  const session = await client.send("sessionGet", { id: created.session.id })
  expect(session.session.connection.mode).toBe("history")
  expect(session.session.errorMessage ?? "").toMatch(/Exited with code 9/i)
  expect(await SessionPermissionsStorage.get(created.session.id)).toBeNull()
})

test("abnormal agent exit invalidates reconnects and repeated shutdowns are harmless", async () => {
  const daemon = await startTestDaemon()
  const client = createDaemonIpcClient({ daemonUrl: daemon.daemonUrl })
  const agentPath = await createAgentScript(`
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
          result: { sessionId: "agent-session-broken" },
        })
        return
      }

      if (message.method === "session/prompt") {
        process.exit(7)
      }
    })
  `)

  const created = await client.send("sessionCreate", {
    agent: createNodeAgent(agentPath),
    cwd: process.cwd(),
    mcpServers: [],
    systemPrompt: "Keep responses short.",
  })

  await client.send("sessionSend", {
    id: created.session.id,
    message: {
      jsonrpc: "2.0",
      id: 1,
      method: "session/prompt",
      params: {
        sessionId: created.session.acpId,
        prompt: [{ type: "text", text: "Trigger failure." }],
      },
    },
  })

  await waitFor(async () => {
    const session = await client.send("sessionGet", { id: created.session.id })
    return session.session.status === "error"
  })

  const session = await client.send("sessionGet", { id: created.session.id })
  expect(session.session.connection.mode).toBe("history")
  expect(session.session.connection.reconnectable).toBe(false)
  expect(session.session.errorMessage ?? "").toMatch(/Exited with code 7/i)

  const diagnostics = await client.send("sessionDiagnostics", { id: created.session.id })
  expect(diagnostics.events.some((event) => event.type === "agent_process_exit")).toBe(true)

  expect((await client.send("sessionShutdown", { id: created.session.id })).success).toBe(false)
  expect((await client.send("sessionShutdown", { id: created.session.id })).success).toBe(false)
})

test("daemon lists recent sessions with a stable paginated cursor", async () => {
  const createdAt = new Date("2026-01-01T00:00:10.000Z")
  sessions.set("sess-a", {
    id: "sess-a",
    acpId: "acp-a",
    status: "done",
    agentName: "node",
    cwd: process.cwd(),
    mcpServers: [],
    metadata: null,
    createdAt,
    updatedAt: new Date("2026-01-01T00:00:10.000Z"),
    errorMessage: null,
    blockedReason: null,
    initiative: "Older",
    lastAgentMessage: "oldest",
  })
  sessions.set("sess-b", {
    id: "sess-b",
    acpId: "acp-b",
    status: "done",
    agentName: "node",
    cwd: process.cwd(),
    mcpServers: [],
    metadata: null,
    createdAt,
    updatedAt: new Date("2026-01-01T00:00:11.000Z"),
    errorMessage: null,
    blockedReason: null,
    initiative: "Middle",
    lastAgentMessage: "middle",
  })
  sessions.set("sess-c", {
    id: "sess-c",
    acpId: "acp-c",
    status: "blocked",
    agentName: "node",
    cwd: process.cwd(),
    mcpServers: [],
    metadata: null,
    createdAt,
    updatedAt: new Date("2026-01-01T00:00:11.000Z"),
    errorMessage: null,
    blockedReason: "Needs review",
    initiative: "Newest",
    lastAgentMessage: "newest",
  })
  sessionStates.set("sess-a", {
    sessionId: "sess-a",
    acpId: "acp-a",
    connectionMode: "history",
    history: [],
    diagnostics: [],
    activeDaemonSession: false,
    createdAt: new Date().toISOString(),
    updatedAt: new Date().toISOString(),
  })
  sessionStates.set("sess-b", {
    sessionId: "sess-b",
    acpId: "acp-b",
    connectionMode: "live",
    history: [{ jsonrpc: "2.0", method: "session/update", params: {} }],
    diagnostics: [
      { type: "session_status_changed", at: new Date().toISOString(), sessionId: "sess-b" },
    ],
    activeDaemonSession: true,
    createdAt: new Date().toISOString(),
    updatedAt: new Date().toISOString(),
  })
  sessionStates.set("sess-c", {
    sessionId: "sess-c",
    acpId: "acp-c",
    connectionMode: "history",
    history: [{ jsonrpc: "2.0", method: "session/update", params: {} }],
    diagnostics: [
      { type: "session_status_changed", at: "2026-01-01T00:00:12.000Z", sessionId: "sess-c" },
      { type: "session_connected", at: "2026-01-01T00:00:13.000Z", sessionId: "sess-c" },
    ],
    activeDaemonSession: false,
    createdAt: new Date().toISOString(),
    updatedAt: new Date().toISOString(),
  })

  const daemon = await startTestDaemon()
  const client = createDaemonIpcClient({ daemonUrl: daemon.daemonUrl })

  const firstPage = await client.send("sessionList", { limit: 2 })
  expect(firstPage.sessions.map((session) => session.id)).toEqual(["sess-c", "sess-b"])
  expect(firstPage.hasMore).toBe(true)
  expect(typeof firstPage.nextCursor).toBe("string")
  expect(firstPage.sessions[0]?.diagnostics.historyLength).toBe(1)
  expect(firstPage.sessions[0]?.connection.mode).toBe("history")

  const secondPage = await client.send("sessionList", {
    limit: 2,
    cursor: firstPage.nextCursor ?? undefined,
  })
  expect(secondPage.sessions.map((session) => session.id)).toEqual(["sess-a"])
  expect(secondPage.hasMore).toBe(false)
  expect(secondPage.nextCursor).toBeNull()
})

test("daemon session listing defaults and caps page size", async () => {
  for (let index = 0; index < 120; index += 1) {
    const id = `sess-${index.toString().padStart(3, "0")}`
    sessions.set(id, {
      id,
      acpId: `acp-${id}`,
      status: "done",
      agentName: "node",
      cwd: process.cwd(),
      mcpServers: [],
      metadata: null,
      createdAt: new Date(1_700_000_000_000 + index * 1_000),
      updatedAt: new Date(1_700_000_000_000 + index * 1_000),
      errorMessage: null,
      blockedReason: null,
      initiative: null,
      lastAgentMessage: null,
    })
    sessionStates.set(id, {
      sessionId: id,
      acpId: `acp-${id}`,
      connectionMode: "history",
      history: [],
      diagnostics: [],
      activeDaemonSession: false,
      createdAt: new Date().toISOString(),
      updatedAt: new Date().toISOString(),
    })
  }

  const daemon = await startTestDaemon()
  const client = createDaemonIpcClient({ daemonUrl: daemon.daemonUrl })

  const defaultPage = await client.send("sessionList", {})
  expect(defaultPage.sessions).toHaveLength(20)
  expect(defaultPage.hasMore).toBe(true)

  const cappedPage = await client.send("sessionList", { limit: 999 })
  expect(cappedPage.sessions).toHaveLength(100)
  expect(cappedPage.hasMore).toBe(true)
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

async function createAgentScript(body: string): Promise<string> {
  const agentDir = await mkdtemp(join(tmpdir(), "goddard-daemon-agent-"))
  const agentPath = join(agentDir, "agent.mjs")

  cleanup.push(async () => {
    await rm(agentDir, { recursive: true, force: true })
  })

  await writeFile(agentPath, `${body.trim()}\n`, "utf-8")
  return agentPath
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

async function captureDaemonLogs<T>(
  action: () => Promise<T>,
): Promise<{ logs: Array<Record<string, unknown>>; result: T }> {
  const output: string[] = []
  const restoreLogging = configureDaemonLogging({ mode: "json" })
  const stdout = vi.spyOn(process.stdout, "write").mockImplementation(((
    chunk: string | Uint8Array,
  ) => {
    output.push(typeof chunk === "string" ? chunk : Buffer.from(chunk).toString("utf-8"))
    return true
  }) as typeof process.stdout.write)

  try {
    const result = await action()
    return {
      logs: output
        .flatMap((chunk) => chunk.split("\n"))
        .filter((line) => line.trim().length > 0)
        .map((line) => JSON.parse(line) as Record<string, unknown>),
      result,
    }
  } finally {
    stdout.mockRestore()
    restoreLogging()
  }
}
