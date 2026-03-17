import * as assert from "node:assert/strict"
import { appendFile, chmod, mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { afterEach, test, vi } from "vitest"
import { watchWorkforce } from "../src/node/workforce.ts"
import { startDaemonServer, type DaemonServer } from "../../../daemon/src/ipc.ts"

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
const originalFetch = globalThis.fetch
const originalLogPath = process.env.WORKFORCE_TEST_PROMPT_LOG
const originalDelayMs = process.env.WORKFORCE_TEST_DELAY_MS

afterEach(async () => {
  sessions.clear()
  sessionStates.clear()
  permissionsBySessionId.clear()
  permissionsByToken.clear()
  globalThis.fetch = originalFetch

  if (originalLogPath === undefined) {
    delete process.env.WORKFORCE_TEST_PROMPT_LOG
  } else {
    process.env.WORKFORCE_TEST_PROMPT_LOG = originalLogPath
  }

  if (originalDelayMs === undefined) {
    delete process.env.WORKFORCE_TEST_DELAY_MS
  } else {
    process.env.WORKFORCE_TEST_DELAY_MS = originalDelayMs
  }

  while (cleanup.length > 0) {
    await cleanup.pop()?.()
  }
})

test("watchWorkforce drives real daemon-backed pi sessions and coalesces follow-up appends", async () => {
  const rootDir = await mkdtemp(join(tmpdir(), "goddard-workforce-integration-"))
  const socketDir = await mkdtemp(join(tmpdir(), "goddard-workforce-socket-"))
  const agentBinDir = await mkdtemp(join(tmpdir(), "goddard-workforce-agent-bin-"))
  const promptLogPath = join(rootDir, "prompts.jsonl")
  const requestsPath = join(rootDir, ".goddard", "requests.jsonl")
  const responsesPath = join(rootDir, ".goddard", "responses.jsonl")

  process.env.WORKFORCE_TEST_PROMPT_LOG = promptLogPath
  process.env.WORKFORCE_TEST_DELAY_MS = "150"

  await writeFile(join(rootDir, "package.json"), JSON.stringify({ name: "@repo/root" }, null, 2))
  await mkdir(join(rootDir, ".goddard"), { recursive: true })
  await writeFile(
    join(agentBinDir, "pi"),
    `#!/usr/bin/env node
import * as fs from "node:fs/promises"
import * as readline from "node:readline"

const delayMs = Number(process.env.WORKFORCE_TEST_DELAY_MS ?? "0")
const logPath = process.env.WORKFORCE_TEST_PROMPT_LOG
let promptCount = 0
const rl = readline.createInterface({ input: process.stdin })

function send(message) {
  process.stdout.write(\`\${JSON.stringify(message)}\\n\`)
}

rl.on("line", async (line) => {
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
      result: { sessionId: "workforce-agent-session" },
    })
    return
  }

  if (message.method === "session/prompt") {
    promptCount += 1
    if (logPath) {
      await fs.appendFile(logPath, JSON.stringify(message.params.prompt) + "\\n", "utf-8")
    }

    if (promptCount === 1 && delayMs > 0) {
      await new Promise((resolve) => setTimeout(resolve, delayMs))
    }

    send({
      jsonrpc: "2.0",
      id: message.id,
      result: { stopReason: "end_turn" },
    })
  }
})
`,
    "utf-8",
  )
  await chmod(join(agentBinDir, "pi"), 0o755)
  await writeFile(requestsPath, "", "utf-8")
  await writeFile(responsesPath, "", "utf-8")

  cleanup.push(async () => {
    await rm(rootDir, { recursive: true, force: true })
    await rm(socketDir, { recursive: true, force: true })
    await rm(agentBinDir, { recursive: true, force: true })
  })

  globalThis.fetch = vi.fn(async (input) => {
    if (String(input).includes("/agents/pi/agent.json")) {
      return new Response(
        JSON.stringify({
          name: "pi",
          version: "test",
          distribution: {
            type: "binary",
            cmd: "pi",
          },
        }),
        { status: 200 },
      )
    }

    throw new Error(`Unexpected fetch request: ${String(input)}`)
  }) as typeof fetch

  const daemon = await startTestDaemon({
    socketPath: join(socketDir, "daemon.sock"),
    agentBinDir,
  })
  const supervisor = await watchWorkforce({
    rootDir,
    daemon: { daemonUrl: daemon.daemonUrl },
  })

  cleanup.push(async () => {
    await supervisor.stop().catch(() => {})
    await daemon.close().catch(() => {})
  })

  await appendFile(requestsPath, '{"id":"first"}\n', "utf-8")
  await waitForLogLines(promptLogPath, 1)

  await appendFile(requestsPath, '{"id":"second"}\n', "utf-8")
  await appendFile(responsesPath, '{"id":"third"}\n', "utf-8")
  await waitForLogLines(promptLogPath, 2)

  const loggedPrompts = (await readFile(promptLogPath, "utf-8"))
    .trim()
    .split("\n")
    .map((line) => JSON.parse(line) as Array<{ text?: string }>)
    .map((blocks) => blocks.map((block) => block.text ?? "").join("\n"))

  assert.match(loggedPrompts[0], /"id":"first"/)
  assert.match(loggedPrompts[1], /"id":"second"/)
  assert.match(loggedPrompts[1], /"id":"third"/)
}, 20_000)

async function startTestDaemon(options: {
  socketPath: string
  agentBinDir: string
}): Promise<DaemonServer> {
  return startDaemonServer(
    {
      pr: {
        create: async () => ({
          number: 1,
          url: "https://github.com/example/repo/pull/1",
        }),
        reply: async () => ({ success: true }),
      },
    },
    options,
  )
}

async function waitForLogLines(logPath: string, lineCount: number, timeoutMs: number = 15_000) {
  const startTime = Date.now()

  while (Date.now() - startTime < timeoutMs) {
    try {
      const lineTotal = (await readFile(logPath, "utf-8"))
        .split("\n")
        .filter((line) => line.trim()).length

      if (lineTotal >= lineCount) {
        return
      }
    } catch {
      // Wait until the log file exists.
    }

    await new Promise((resolve) => setTimeout(resolve, 25))
  }

  throw new Error("Timed out waiting for prompt log output")
}
