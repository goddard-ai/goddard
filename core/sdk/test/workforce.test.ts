import { execFileSync } from "node:child_process"
import * as fs from "node:fs/promises"
import * as os from "node:os"
import * as path from "node:path"
import { assert, beforeEach, test, vi } from "vitest"

const {
  cancelDaemonWorkforceRequestMock,
  createDaemonWorkforceRequestMock,
  getDaemonWorkforceMock,
  listDaemonWorkforcesMock,
  shutdownDaemonWorkforceMock,
  startDaemonWorkforceMock,
  truncateDaemonWorkforceMock,
  updateDaemonWorkforceRequestMock,
} = vi.hoisted(() => ({
  cancelDaemonWorkforceRequestMock: vi.fn(),
  createDaemonWorkforceRequestMock: vi.fn(),
  getDaemonWorkforceMock: vi.fn(),
  listDaemonWorkforcesMock: vi.fn(),
  shutdownDaemonWorkforceMock: vi.fn(),
  startDaemonWorkforceMock: vi.fn(),
  truncateDaemonWorkforceMock: vi.fn(),
  updateDaemonWorkforceRequestMock: vi.fn(),
}))

vi.mock("../src/daemon/workforce.js", async (importOriginal): Promise<typeof import("../src/daemon/workforce.js")> => ({
  ...(await importOriginal<typeof import("../src/daemon/workforce.js")>()),
  cancelDaemonWorkforceRequest: cancelDaemonWorkforceRequestMock,
  createDaemonWorkforceRequest: createDaemonWorkforceRequestMock,
  getDaemonWorkforce: getDaemonWorkforceMock,
  listDaemonWorkforces: listDaemonWorkforcesMock,
  shutdownDaemonWorkforce: shutdownDaemonWorkforceMock,
  startDaemonWorkforce: startDaemonWorkforceMock,
  truncateDaemonWorkforce: truncateDaemonWorkforceMock,
  updateDaemonWorkforceRequest: updateDaemonWorkforceRequestMock,
}))

beforeEach(() => {
  cancelDaemonWorkforceRequestMock.mockReset()
  createDaemonWorkforceRequestMock.mockReset()
  getDaemonWorkforceMock.mockReset()
  listDaemonWorkforcesMock.mockReset()
  shutdownDaemonWorkforceMock.mockReset()
  startDaemonWorkforceMock.mockReset()
  truncateDaemonWorkforceMock.mockReset()
  updateDaemonWorkforceRequestMock.mockReset()
})

test("resolveRepositoryRoot returns the nearest git root", async () => {
  const rootDir = await fs.mkdtemp(path.join(os.tmpdir(), "goddard-workforce-root-"))
  const nestedDir = path.join(rootDir, "packages", "worker")

  await fs.mkdir(nestedDir, { recursive: true })
  runGit(rootDir, ["init"])

  const { resolveRepositoryRoot } = await import("../src/node/workforce.ts")
  assert.equal(
    await fs.realpath(await resolveRepositoryRoot(nestedDir)),
    await fs.realpath(rootDir),
  )
})

test("discoverWorkforceInitCandidates returns nested packages under the repository root", async () => {
  const rootDir = await fs.mkdtemp(path.join(os.tmpdir(), "goddard-workforce-candidates-"))

  await writePackageJson(rootDir, "@repo/root")
  await writePackageJson(path.join(rootDir, "packages", "ui"), "@repo/ui")
  await writePackageJson(path.join(rootDir, "packages", "api"), "@repo/api")
  await fs.mkdir(path.join(rootDir, "node_modules", "ignored"), { recursive: true })

  const { discoverWorkforceInitCandidates } = await import("../src/node/workforce.ts")
  const candidates = await discoverWorkforceInitCandidates(rootDir)

  assert.deepEqual(
    candidates.map((pkg) => `${pkg.name}:${pkg.relativeDir}`),
    ["@repo/root:.", "@repo/api:packages/api", "@repo/ui:packages/ui"],
  )
})

test("initializeWorkforce creates root config and ledger files", async () => {
  const rootDir = await fs.mkdtemp(path.join(os.tmpdir(), "goddard-workforce-init-"))
  const uiDir = path.join(rootDir, "packages", "ui")

  await writePackageJson(rootDir, "@repo/root")
  await writePackageJson(uiDir, "@repo/ui")

  const { initializeWorkforce } = await import("../src/node/workforce.ts")
  const initialized = await initializeWorkforce(rootDir, [rootDir, uiDir])
  const config = JSON.parse(await fs.readFile(initialized.configPath, "utf-8")) as {
    rootAgentId: string
    agents: Array<{ id: string; cwd: string }>
  }

  assert.equal(config.rootAgentId, "root")
  assert.deepEqual(config.agents.map((agent) => agent.cwd).sort(), [".", "packages/ui"])
  assert.equal(await fs.readFile(initialized.ledgerPath, "utf-8"), "")
})

test("node workforce helpers delegate lifecycle and request mutations to daemon wrappers", async () => {
  startDaemonWorkforceMock.mockResolvedValue({ rootDir: "/repo" })
  getDaemonWorkforceMock.mockResolvedValue({ rootDir: "/repo" })
  listDaemonWorkforcesMock.mockResolvedValue([{ rootDir: "/repo" }])
  shutdownDaemonWorkforceMock.mockResolvedValue(true)
  createDaemonWorkforceRequestMock.mockResolvedValue({ requestId: "req-1" })
  updateDaemonWorkforceRequestMock.mockResolvedValue({ requestId: "req-1" })
  cancelDaemonWorkforceRequestMock.mockResolvedValue({ requestId: "req-1" })
  truncateDaemonWorkforceMock.mockResolvedValue({ requestId: null })

  const workforce = await import("../src/node/workforce.ts")

  await workforce.startWorkforce("/repo", {
    daemonUrl: "http://unix/?socketPath=%2Ftmp%2Fdaemon.sock",
  })
  await workforce.getWorkforce("/repo")
  await workforce.listWorkforces()
  await workforce.stopWorkforce("/repo")
  await workforce.createWorkforceRequest({
    rootDir: "/repo",
    targetAgentId: "api",
    message: "Ship the change.",
  })
  await workforce.updateWorkforceRequest({
    rootDir: "/repo",
    requestId: "req-1",
    message: "Resume with the new decision.",
  })
  await workforce.cancelWorkforceRequest({
    rootDir: "/repo",
    requestId: "req-1",
    reason: "No longer needed.",
  })
  await workforce.truncateWorkforce({
    rootDir: "/repo",
    agentId: "api",
    reason: "Reset the failed branch.",
  })

  assert.equal(startDaemonWorkforceMock.mock.calls.length, 1)
  assert.equal(getDaemonWorkforceMock.mock.calls.length, 1)
  assert.equal(listDaemonWorkforcesMock.mock.calls.length, 1)
  assert.equal(shutdownDaemonWorkforceMock.mock.calls.length, 1)
  assert.equal(createDaemonWorkforceRequestMock.mock.calls.length, 1)
  assert.equal(updateDaemonWorkforceRequestMock.mock.calls.length, 1)
  assert.equal(cancelDaemonWorkforceRequestMock.mock.calls.length, 1)
  assert.equal(truncateDaemonWorkforceMock.mock.calls.length, 1)
})

async function writePackageJson(directory: string, name: string): Promise<void> {
  await fs.mkdir(directory, { recursive: true })
  await fs.writeFile(
    path.join(directory, "package.json"),
    JSON.stringify({ name }, null, 2),
    "utf-8",
  )
}

function runGit(cwd: string, args: string[]): void {
  execFileSync("git", args, { cwd, stdio: "pipe" })
}
