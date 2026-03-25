import { mkdtemp, rm } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { afterEach, expect, test, vi } from "vitest"

const cleanup: Array<() => Promise<void>> = []

afterEach(async () => {
  vi.resetModules()

  while (cleanup.length > 0) {
    await cleanup.pop()?.()
  }
})

test("real storage reconciliation marks interrupted sessions as archived history on boot", async () => {
  const homeDir = await mkdtemp(join(tmpdir(), "goddard-real-storage-home-"))
  const socketDir = await mkdtemp(join(tmpdir(), "goddard-real-storage-socket-"))
  const socketPath = join(socketDir, "daemon.sock")
  const previousHome = process.env.HOME

  cleanup.push(async () => {
    process.env.HOME = previousHome
    await rm(homeDir, { recursive: true, force: true })
    await rm(socketDir, { recursive: true, force: true })
  })

  process.env.HOME = homeDir

  const worktreeCleanupMock = vi.fn(() => true)
  vi.doMock("@goddard-ai/worktree", () => ({
    Worktree: class {
      poweredBy = "mock-worktree"

      setup() {
        throw new Error("setup should not run during reconciliation")
      }

      cleanup(worktreeDir: string, branchName: string) {
        return worktreeCleanupMock(worktreeDir, branchName)
      }
    },
  }))

  const [{ startDaemonServer }, { createDaemonIpcClient }, storage] = await Promise.all([
    import("../src/ipc.ts"),
    import("@goddard-ai/daemon-client"),
    import("../src/persistence/index.ts"),
  ])

  const sessionId = "real-restart-session-1"
  await storage.SessionStorage.create({
    id: sessionId,
    acpId: "real-acp-session-1",
    status: "active",
    agentName: "pi",
    cwd: process.cwd(),
    mcpServers: [],
    metadata: {
      worktree: {
        repoRoot: homeDir,
        requestedCwd: homeDir,
        effectiveCwd: "/tmp/mock-worktree/session-real-restart-session-1",
        worktreeDir: "/tmp/mock-worktree/session-real-restart-session-1",
        branchName: "session-real-restart-session-1",
        poweredBy: "mock-worktree",
      },
    },
  })
  await storage.SessionStateStorage.create({
    sessionId,
    acpId: "real-acp-session-1",
    connectionMode: "live",
    history: [{ jsonrpc: "2.0", method: "session/update", params: { text: "persisted" } }],
    diagnostics: [],
    activeDaemonSession: true,
  })
  await storage.SessionPermissionsStorage.create({
    sessionId,
    token: "real-token-1",
    owner: "acme",
    repo: "widgets",
    allowedPrNumbers: [12],
  })

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
  })

  const client = createDaemonIpcClient({ daemonUrl: daemon.daemonUrl })
  const session = await client.send("sessionGet", { id: sessionId })
  expect(session.session.status).toBe("error")
  expect(session.session.connection.mode).toBe("history")
  expect(session.session.connection.reconnectable).toBe(false)
  expect(session.session.errorMessage ?? "").toMatch(/previous daemon exited unexpectedly/i)

  const history = await client.send("sessionHistory", { id: sessionId })
  expect(history.history).toHaveLength(1)

  const diagnostics = await client.send("sessionDiagnostics", { id: sessionId })
  expect(
    diagnostics.events.some((event) => event.type === "session_reconciled_after_restart"),
  ).toBe(true)
  expect(worktreeCleanupMock).not.toHaveBeenCalled()

  await expect(client.send("sessionResolveToken", { token: "real-token-1" })).rejects.toThrow(
    /invalid session token/i,
  )
})
