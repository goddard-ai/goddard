import { createDaemonIpcClient } from "@goddard-ai/daemon-client/node"
import { createRequire } from "node:module"
import { spawnSync } from "node:child_process"
import { existsSync } from "node:fs"
import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { afterAll, afterEach, expect, test } from "vitest"
import { createWrappedNodeAgent } from "./acp-fixture.ts"
import { startDaemonServer, type DaemonServer } from "../src/ipc.ts"
import {
  SessionPermissionsStorage,
  SessionStateStorage,
  SessionStorage,
} from "../src/persistence/index.ts"

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

  const permissions = await SessionPermissionsStorage.get(created.session.id)
  expect(permissions).toBeTruthy()
  expect(typeof permissions?.token).toBe("string")

  await client.send("sessionShutdown", { id: created.session.id })

  await waitFor(async () => {
    return (await SessionPermissionsStorage.getByToken(permissions!.token)) === null
  })

  expect(await SessionPermissionsStorage.getByToken(permissions!.token)).toBeNull()
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
    metadata: {
      workforce: { agentId: "reviewer", requestId: "req-1" },
    },
  })

  const stored = await SessionStorage.get(created.session.id)

  expect(created.session.repository).toBe("acme/widgets")
  expect(created.session.prNumber).toBe(12)
  expect(stored).toMatchObject({
    repository: "acme/widgets",
    prNumber: 12,
    metadata: {
      workforce: { agentId: "reviewer", requestId: "req-1" },
    },
  })

  await client.send("sessionShutdown", { id: created.session.id })
})

test("daemon reconciles interrupted sessions on restart and leaves archived history readable", async () => {
  await useTempHome()

  const sessionId = "session-restart-1"
  await SessionStorage.create({
    id: sessionId,
    acpId: "acp-restart-1",
    status: "active",
    agentName: "node",
    cwd: process.cwd(),
    mcpServers: [],
    metadata: null,
  })
  await SessionStateStorage.create({
    sessionId,
    acpId: "acp-restart-1",
    connectionMode: "live",
    history: [{ jsonrpc: "2.0", method: "session/update", params: { value: "persisted" } }],
    diagnostics: [],
    activeDaemonSession: true,
  })
  await SessionPermissionsStorage.create({
    sessionId,
    token: "tok-restart-1",
    owner: "acme",
    repo: "widgets",
    allowedPrNumbers: [12],
  })

  const daemon = await startTestDaemon({ useExistingHome: true })
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

  const worktree = created.session.metadata?.worktree
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
