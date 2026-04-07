import { createDaemonIpcClient } from "@goddard-ai/daemon-client/node"
import { afterAll, afterEach, expect, test } from "bun:test"
import { spawnSync } from "node:child_process"
import { randomUUID } from "node:crypto"
import { existsSync } from "node:fs"
import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises"
import { createRequire } from "node:module"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { startDaemonServer, type DaemonServer } from "../src/ipc.ts"
import { db, resetDb } from "../src/persistence/store.ts"
import { createWrappedNodeAgent } from "./acp-fixture.ts"

const cleanup: Array<() => Promise<void>> = []
const originalHome = process.env.HOME
let sharedHomeDir: string | null = null

afterEach(async () => {
  while (cleanup.length > 0) {
    await cleanup.pop()?.()
  }
})

afterAll(async () => {
  if (originalHome === undefined) {
    delete process.env.HOME
  } else {
    process.env.HOME = originalHome
  }
  resetDb()

  if (sharedHomeDir) {
    await rm(sharedHomeDir, { recursive: true, force: true })
  }
})

test("daemon revokes session tokens when agent processes exit", async () => {
  const daemon = await startTestDaemon()
  const client = createDaemonIpcClient({ daemonUrl: daemon.daemonUrl })
  const require = createRequire(import.meta.url)
  const exampleAgentPath = require.resolve("@agentclientprotocol/sdk/dist/examples/agent.js")

  const created = await client.send("sessionCreate", {
    agent: createWrappedNodeAgent(exampleAgentPath),
    cwd: process.cwd(),
    mcpServers: [],
    systemPrompt: "Keep responses short.",
  })

  const permissions = db.sessions.get(created.session.id)?.permissions ?? null
  const token = db.sessions.get(created.session.id)?.token ?? null
  expect(permissions).toBeTruthy()
  expect(typeof token).toBe("string")

  await client.send("sessionShutdown", { id: created.session.id })

  await waitFor(async () => {
    return db.sessions.get(created.session.id)?.permissions == null
  })

  expect(db.sessions.get(created.session.id)?.permissions ?? null).toBeNull()
})

test("daemon persists repository context into durable session storage", async () => {
  const daemon = await startTestDaemon()
  const client = createDaemonIpcClient({ daemonUrl: daemon.daemonUrl })
  const require = createRequire(import.meta.url)
  const exampleAgentPath = require.resolve("@agentclientprotocol/sdk/dist/examples/agent.js")

  const created = await client.send("sessionCreate", {
    agent: createWrappedNodeAgent(exampleAgentPath),
    cwd: process.cwd(),
    mcpServers: [],
    systemPrompt: "Keep responses short.",
    repository: "acme/widgets",
    prNumber: 12,
    workforce: { agentId: "reviewer", requestId: "req-1" },
  })

  const storedRecord = db.sessions.get(created.session.id) ?? null
  const workforceRecord =
    db.workforces.first({
      where: { sessionId: created.session.id },
    }) ?? null
  const workforce = await client.send("sessionWorkforce", { id: created.session.id })

  expect(created.session.repository).toBe("acme/widgets")
  expect(created.session.prNumber).toBe(12)
  expect(storedRecord).toMatchObject({
    repository: "acme/widgets",
    prNumber: 12,
  })
  expect(storedRecord?.metadata ?? null).toBeNull()
  expect(workforceRecord).toMatchObject({
    sessionId: created.session.id,
    agentId: "reviewer",
    requestId: "req-1",
  })
  expect(created.session.metadata ?? null).toBeNull()
  expect(workforce.workforce).toMatchObject({
    agentId: "reviewer",
    requestId: "req-1",
  })

  await client.send("sessionShutdown", { id: created.session.id })
})

test("daemon reconciles interrupted sessions on restart and leaves archived history readable", async () => {
  await useTempHome()

  const sessionId = db.sessions.newId()
  const acpSessionId = `acp-restart-${randomUUID()}`
  const sessionRecord = {
    acpSessionId,
    status: "active",
    agentName: "node",
    cwd: process.cwd(),
    mcpServers: [],
    connectionMode: "live",
    activeDaemonSession: true,
    errorMessage: null,
    blockedReason: null,
    initiative: null,
    lastAgentMessage: null,
    repository: null,
    prNumber: null,
    token: "tok-restart-1",
    permissions: {
      owner: "acme",
      repo: "widgets",
      allowedPrNumbers: [12],
    },
    metadata: null,
    models: null,
  } satisfies Parameters<typeof db.sessions.put>[1]
  db.sessions.put(sessionId, sessionRecord)
  db.sessionMessages.create({
    sessionId,
    messages: [{ jsonrpc: "2.0", method: "session/update", params: { value: "persisted" } }],
  })
  db.sessionDiagnostics.create({
    sessionId,
    events: [],
  })
  const daemon = await startTestDaemon({ useExistingHome: true })
  const client = createDaemonIpcClient({ daemonUrl: daemon.daemonUrl })

  const session = await client.send("sessionGet", { id: sessionId })
  expect(session.session.status).toBe("error")
  expect(session.session.connectionMode).toBe("history")
  expect(session.session.activeDaemonSession).toBe(false)
  expect(session.session.errorMessage ?? "").toMatch(/previous daemon exited unexpectedly/i)

  const history = await client.send("sessionHistory", { id: sessionId })
  expect(history.connection.mode).toBe("history")
  expect(history.history).toHaveLength(1)

  const diagnostics = await client.send("sessionDiagnostics", { id: sessionId })
  expect(
    diagnostics.events.some((event) => event.type === "session_reconciled_after_restart"),
  ).toBe(true)
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
    agent: createWrappedNodeAgent(exampleAgentPath),
    cwd: process.cwd(),
    mcpServers: [],
    systemPrompt: "Keep responses short.",
  })

  const clientAMessages: unknown[] = []
  const clientBMessages: unknown[] = []
  const unsubscribeA = await clientA.subscribe(
    { name: "sessionMessage", filter: { id: created.session.id } },
    (payload) => {
      clientAMessages.push(payload.message)
    },
  )
  const unsubscribeB = await clientB.subscribe(
    { name: "sessionMessage", filter: { id: created.session.id } },
    (payload) => {
      clientBMessages.push(payload.message)
    },
  )

  await clientA.send("sessionSend", {
    id: created.session.id,
    message: {
      jsonrpc: "2.0",
      id: 1,
      method: "session/prompt",
      params: {
        sessionId: created.session.acpSessionId,
        prompt: [{ type: "text", text: "Say hello in one sentence." }],
      },
    },
  })

  await waitFor(async () => clientAMessages.length > 0 && clientBMessages.length > 0)

  await Promise.resolve(unsubscribeA()).catch(() => {})
  await Promise.resolve(unsubscribeB()).catch(() => {})

  expect(clientAMessages.length).toBeGreaterThan(0)
  expect(clientBMessages.length).toBeGreaterThan(0)
})

test("session worktree opt-in maps cwd into a real worktree subdirectory", async () => {
  const daemon = await startTestDaemon()
  const client = createDaemonIpcClient({ daemonUrl: daemon.daemonUrl })
  const require = createRequire(import.meta.url)
  const exampleAgentPath = require.resolve("@agentclientprotocol/sdk/dist/examples/agent.js")
  const repoDir = await createRepoFixture({ includeSrc: true })
  const requestedCwd = join(repoDir, "src")

  const created = await client.send("sessionCreate", {
    agent: createWrappedNodeAgent(exampleAgentPath),
    cwd: requestedCwd,
    worktree: { enabled: true },
    mcpServers: [],
    systemPrompt: "Keep responses short.",
  })

  const worktree = (await client.send("sessionWorktree", { id: created.session.id })).worktree
  expect(worktree).toBeTruthy()
  expect(worktree?.requestedCwd.endsWith("/src")).toBe(true)
  expect(worktree?.effectiveCwd).toMatch(/\/src$/)
  expect(worktree?.worktreeDir).not.toBe(repoDir)
  expect(existsSync(worktree!.worktreeDir)).toBe(true)
  expect(existsSync(worktree!.effectiveCwd)).toBe(true)
  await client.send("sessionShutdown", { id: created.session.id })
})

async function startTestDaemon(options: { useExistingHome?: boolean } = {}): Promise<DaemonServer> {
  if (!options.useExistingHome) {
    await useTempHome()
  }

  const socketDir = await mkdtemp(join(tmpdir(), "goddard-daemon-session-"))
  const socketPath = join(socketDir, "daemon.sock")

  const daemon = await startDaemonServer(
    {
      auth: {
        startDeviceFlow: async () => ({
          deviceCode: "dev_1",
          userCode: "ABCD-1234",
          verificationUri: "https://github.com/login/device",
          expiresIn: 900,
          interval: 5,
        }),
        completeDeviceFlow: async () => ({
          token: "tok_1",
          githubUsername: "alec",
          githubUserId: 42,
        }),
        whoami: async () => ({ token: "tok_1", githubUsername: "alec", githubUserId: 42 }),
        logout: async () => {},
      },
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

async function useTempHome(): Promise<void> {
  sharedHomeDir ??= await mkdtemp(join(tmpdir(), "goddard-daemon-home-"))
  process.env.HOME = sharedHomeDir
  resetDb()
}

async function createRepoFixture(options: { includeSrc?: boolean } = {}): Promise<string> {
  const repoDir = await mkdtemp(join(tmpdir(), "goddard-daemon-repo-"))
  cleanup.push(async () => {
    await rm(repoDir, { recursive: true, force: true })
  })

  await writeFile(
    join(repoDir, "package.json"),
    JSON.stringify({ name: "repo", private: true }, null, 2),
    "utf-8",
  )

  if (options.includeSrc) {
    await mkdir(join(repoDir, "src"), { recursive: true })
    await writeFile(join(repoDir, "src", "index.ts"), "export const ready = true\n", "utf-8")
  }

  runGit(repoDir, ["init"])
  runGit(repoDir, ["config", "user.email", "bot@example.com"])
  runGit(repoDir, ["config", "user.name", "Bot"])
  runGit(repoDir, ["add", "."])
  runGit(repoDir, ["commit", "-m", "init"])

  return repoDir
}

function runGit(cwd: string, args: string[]) {
  const result = spawnSync("git", args, {
    cwd,
    encoding: "utf-8",
  })

  expect(result.status).toBe(0)
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
