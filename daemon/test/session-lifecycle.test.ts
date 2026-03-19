import { afterEach, test, vi } from "vitest"
import * as assert from "node:assert/strict"
import { mkdtemp, rm, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { createRequire } from "node:module"
import { configureDaemonLogging } from "../src/logging.ts"

const { permissionsBySessionId, permissionsByToken, sessionStates, sessions } = vi.hoisted(() => ({
  sessions: new Map<string, any>(),
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

import { createDaemonIpcClient } from "@goddard-ai/daemon-client"
import { SessionPermissionsStorage } from "@goddard-ai/storage/session-permissions"
import { startDaemonServer, type DaemonServer } from "../src/ipc.ts"

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
  assert.ok(permissions)
  assert.equal(typeof permissions.token, "string")

  await client.send("sessionShutdown", { id: created.session.id })

  await waitFor(async () => {
    return (await SessionPermissionsStorage.getByToken(permissions.token)) === null
  })

  assert.equal(await SessionPermissionsStorage.getByToken(permissions.token), null)
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
    metadata: null,
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
  assert.equal(session.session.status, "error")
  assert.equal(session.session.connection.mode, "history")
  assert.equal(session.session.connection.reconnectable, false)
  assert.match(session.session.errorMessage ?? "", /previous daemon exited unexpectedly/i)

  const history = await client.send("sessionHistory", { id: sessionId })
  assert.equal(history.connection.mode, "history")
  assert.equal(history.history.length, 1)

  const diagnostics = await client.send("sessionDiagnostics", { id: sessionId })
  assert.equal(
    diagnostics.events.some((event) => event.type === "session_reconciled_after_restart"),
    true,
  )
  await assert.rejects(() => client.send("sessionConnect", { id: sessionId }), /archived/i)
  await assert.rejects(
    () => client.send("sessionResolveToken", { token: "tok-restart-1" }),
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
  assert.equal(clientAMessages.length > 0, true)
  assert.equal(clientBMessages.length > 0, true)
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
  assert.ok(writeLog)
  assert.equal(writeLog?.acpId, result.acpId)
  assert.equal(writeLog?.hasId, true)
  assert.deepEqual((writeLog.message as any).params.prompt[1].text, {
    text: `${longPrompt.slice(0, 512)}...`,
    byteLength: Buffer.byteLength(longPrompt),
    truncated: true,
  })

  const readLog = logs.find(
    (entry) => entry.event === "agent.message_read" && entry.sessionId === result.sessionId,
  )
  assert.ok(readLog)
  assert.equal(readLog?.acpId, result.acpId)
  assert.equal(readLog?.hasId, true)
  assert.deepEqual((readLog.message as any).result.content[0].text, {
    text: `${longReply.slice(0, 512)}...`,
    byteLength: Buffer.byteLength(longReply),
    truncated: true,
  })

  const chunkLog = logs.find(
    (entry) =>
      entry.event === "agent.chunk_read" &&
      entry.sessionId === result.sessionId &&
      (entry.preview as any)?.truncated === true,
  )
  assert.ok(chunkLog)
  assert.equal(chunkLog?.acpId, result.acpId)
  assert.equal(typeof (chunkLog.preview as any).text, "string")
  assert.equal((chunkLog.preview as any).truncated, true)
  assert.equal((chunkLog.preview as any).byteLength > 256, true)

  const diagnostics = await client.send("sessionDiagnostics", { id: result.sessionId })
  assert.equal(
    diagnostics.events.some(
      (event) => event.type === "agent.message_read" || event.type === "agent.chunk_read",
    ),
    false,
  )
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
  assert.equal(session.session.connection.mode, "history")
  assert.match(session.session.errorMessage ?? "", /Exited with code 9/i)
  assert.equal(await SessionPermissionsStorage.get(created.session.id), null)
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
  assert.equal(session.session.connection.mode, "history")
  assert.equal(session.session.connection.reconnectable, false)
  assert.match(session.session.errorMessage ?? "", /Exited with code 7/i)

  const diagnostics = await client.send("sessionDiagnostics", { id: created.session.id })
  assert.equal(
    diagnostics.events.some((event) => event.type === "agent_process_exit"),
    true,
  )

  assert.equal((await client.send("sessionShutdown", { id: created.session.id })).success, false)
  assert.equal((await client.send("sessionShutdown", { id: created.session.id })).success, false)
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
