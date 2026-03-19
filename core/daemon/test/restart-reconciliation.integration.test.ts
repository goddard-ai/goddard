import { mkdtemp, rm } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { afterEach, assert, test, vi } from "vitest"

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

  const [{ startDaemonServer }, { createDaemonIpcClient }, storage] = await Promise.all([
    import("../src/ipc.ts"),
    import("@goddard-ai/daemon-client"),
    import("@goddard-ai/storage"),
  ])

  const sessionId = "real-restart-session-1"
  await storage.SessionStorage.create({
    id: sessionId,
    acpId: "real-acp-session-1",
    status: "active",
    agentName: "pi",
    cwd: process.cwd(),
    mcpServers: [],
    metadata: null,
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
  assert.equal(session.session.status, "error")
  assert.equal(session.session.connection.mode, "history")
  assert.equal(session.session.connection.reconnectable, false)
  assert.match(session.session.errorMessage ?? "", /previous daemon exited unexpectedly/i)

  const history = await client.send("sessionHistory", { id: sessionId })
  assert.equal(history.history.length, 1)

  const diagnostics = await client.send("sessionDiagnostics", { id: sessionId })
  assert.equal(
    diagnostics.events.some((event) => event.type === "session_reconciled_after_restart"),
    true,
  )

  await assert.rejects(
    () => client.send("sessionResolveToken", { token: "real-token-1" }),
    /invalid session token/i,
  )
})
