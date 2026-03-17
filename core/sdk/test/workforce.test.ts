import * as assert from "node:assert/strict"
import { execFileSync } from "node:child_process"
import * as fs from "node:fs/promises"
import * as os from "node:os"
import * as path from "node:path"
import { afterEach, beforeEach, test, vi } from "vitest"

const { promptMock, runAgentMock, stopMock } = vi.hoisted(() => ({
  promptMock: vi.fn(),
  stopMock: vi.fn(async () => {}),
  runAgentMock: vi.fn(async ({ cwd }: { cwd: string }) => ({
    sessionId: `session:${cwd}`,
    prompt: promptMock,
    stop: stopMock,
  })),
}))

vi.mock("../src/daemon/session/client.ts", () => ({
  runAgent: runAgentMock,
}))

const supervisors: Array<{ stop: () => Promise<void> }> = []

beforeEach(() => {
  promptMock.mockReset()
  stopMock.mockClear()
  runAgentMock.mockClear()
})

afterEach(async () => {
  while (supervisors.length > 0) {
    await supervisors.pop()?.stop()
  }
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

test("discoverWorkforceInitCandidates includes the repository root agent and skips initialized packages", async () => {
  const rootDir = await fs.mkdtemp(path.join(os.tmpdir(), "goddard-workforce-candidates-"))

  await fs.mkdir(path.join(rootDir, "packages", "ready", ".goddard"), { recursive: true })
  await fs.mkdir(path.join(rootDir, "packages", "newcomer"), { recursive: true })
  await fs.mkdir(path.join(rootDir, "dist", "ignored"), { recursive: true })
  await fs.mkdir(path.join(rootDir, "node_modules", "ignored"), { recursive: true })

  await writePackageJson(rootDir, "@repo/root")
  await writePackageJson(path.join(rootDir, "packages", "ready"), "@repo/ready")
  await writePackageJson(path.join(rootDir, "packages", "newcomer"), "@repo/newcomer")
  await writePackageJson(path.join(rootDir, "dist", "ignored"), "@repo/ignored-dist")
  await writePackageJson(path.join(rootDir, "node_modules", "ignored"), "@repo/ignored-node")
  await fs.writeFile(
    path.join(rootDir, "packages", "ready", ".goddard", "requests.jsonl"),
    "",
    "utf-8",
  )

  const { discoverWorkforceInitCandidates } = await import("../src/node/workforce.ts")
  const candidates = await discoverWorkforceInitCandidates(rootDir)

  assert.deepEqual(
    candidates.map((pkg) => `${pkg.name}:${pkg.relativeDir}`),
    ["@repo/root:.", "@repo/newcomer:packages/newcomer"],
  )
})

test("initializeWorkforcePackages creates requests and responses idempotently", async () => {
  const rootDir = await fs.mkdtemp(path.join(os.tmpdir(), "goddard-workforce-init-"))
  const packageDir = path.join(rootDir, "packages", "alpha")

  await fs.mkdir(packageDir, { recursive: true })

  const { initializeWorkforcePackages } = await import("../src/node/workforce.ts")
  const first = await initializeWorkforcePackages([packageDir])
  const second = await initializeWorkforcePackages([packageDir])

  assert.deepEqual(first[0].createdPaths.map((createdPath) => path.basename(createdPath)).sort(), [
    "requests.jsonl",
    "responses.jsonl",
  ])
  assert.deepEqual(second[0].createdPaths, [])
  assert.equal(await fs.readFile(path.join(packageDir, ".goddard", "requests.jsonl"), "utf-8"), "")
  assert.equal(await fs.readFile(path.join(packageDir, ".goddard", "responses.jsonl"), "utf-8"), "")
})

test("watchWorkforce ignores backlog, then prompts and coalesces later appends", async () => {
  const rootDir = await fs.mkdtemp(path.join(os.tmpdir(), "goddard-workforce-watch-"))
  const packageDir = path.join(rootDir, "packages", "alpha")
  const requestsPath = path.join(packageDir, ".goddard", "requests.jsonl")
  const firstPrompt = { release: undefined as undefined | (() => void) }

  await fs.mkdir(path.dirname(requestsPath), { recursive: true })
  await writePackageJson(rootDir, "@repo/root")
  await writePackageJson(packageDir, "@repo/alpha")
  await fs.writeFile(requestsPath, '{"id":"backlog"}\n', "utf-8")
  await fs.writeFile(path.join(packageDir, ".goddard", "responses.jsonl"), "", "utf-8")

  promptMock.mockImplementationOnce(
    () =>
      new Promise<void>((resolve) => {
        firstPrompt.release = resolve
      }),
  )
  promptMock.mockResolvedValue(undefined)

  const { watchWorkforce } = await import("../src/node/workforce.ts")
  const supervisor = await watchWorkforce({ rootDir })
  supervisors.push(supervisor)

  await waitFor(() => pathExists(path.join(packageDir, ".goddard", "processed-at.json")))
  assert.equal(promptMock.mock.calls.length, 0)

  await fs.appendFile(requestsPath, '{"id":"first"}\n', "utf-8")
  await waitFor(() => promptMock.mock.calls.length === 1)
  assert.match(promptMock.mock.calls[0][0], /"id":"first"/)
  assert.doesNotMatch(promptMock.mock.calls[0][0], /"id":"backlog"/)

  await fs.appendFile(requestsPath, '{"id":"second"}\n', "utf-8")
  await fs.appendFile(
    path.join(packageDir, ".goddard", "responses.jsonl"),
    '{"id":"third"}\n',
    "utf-8",
  )

  await sleep(100)
  assert.equal(promptMock.mock.calls.length, 1)

  assert.ok(firstPrompt.release)
  firstPrompt.release()
  await waitFor(() => promptMock.mock.calls.length === 2)
  assert.match(promptMock.mock.calls[1][0], /"id":"second"/)
  assert.match(promptMock.mock.calls[1][0], /"id":"third"/)
})

test("watchWorkforce resumes from processed offsets and reseeds after rewrites", async () => {
  const rootDir = await fs.mkdtemp(path.join(os.tmpdir(), "goddard-workforce-resume-"))
  const requestsPath = path.join(rootDir, ".goddard", "requests.jsonl")
  const responsesPath = path.join(rootDir, ".goddard", "responses.jsonl")

  await fs.mkdir(path.dirname(requestsPath), { recursive: true })
  await writePackageJson(rootDir, "@repo/root")
  await fs.writeFile(requestsPath, "", "utf-8")
  await fs.writeFile(responsesPath, "", "utf-8")

  promptMock.mockResolvedValue(undefined)

  const { watchWorkforce } = await import("../src/node/workforce.ts")
  const firstSupervisor = await watchWorkforce({ rootDir })
  supervisors.push(firstSupervisor)

  await fs.appendFile(requestsPath, '{"id":"one"}', "utf-8")
  await sleep(100)
  assert.equal(promptMock.mock.calls.length, 0)

  await fs.appendFile(requestsPath, "\n", "utf-8")
  await waitFor(() => promptMock.mock.calls.length === 1)
  assert.match(promptMock.mock.calls[0][0], /"id":"one"/)

  await firstSupervisor.stop()
  supervisors.pop()
  promptMock.mockClear()

  const secondSupervisor = await watchWorkforce({ rootDir })
  supervisors.push(secondSupervisor)
  await sleep(100)
  assert.equal(promptMock.mock.calls.length, 0)

  await fs.appendFile(requestsPath, '{"id":"two"}\n', "utf-8")
  await waitFor(() => promptMock.mock.calls.length === 1)
  assert.match(promptMock.mock.calls[0][0], /"id":"two"/)
  assert.doesNotMatch(promptMock.mock.calls[0][0], /"id":"one"/)

  promptMock.mockClear()
  await fs.writeFile(requestsPath, '{"id":"rewritten"}\n', "utf-8")
  await sleep(150)
  assert.equal(promptMock.mock.calls.length, 0)

  await fs.appendFile(requestsPath, '{"id":"after-rewrite"}\n', "utf-8")
  await waitFor(() => promptMock.mock.calls.length === 1)
  assert.match(promptMock.mock.calls[0][0], /"id":"after-rewrite"/)
  assert.doesNotMatch(promptMock.mock.calls[0][0], /"id":"rewritten"/)
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

async function pathExists(targetPath: string): Promise<boolean> {
  try {
    await fs.stat(targetPath)
    return true
  } catch {
    return false
  }
}

async function waitFor(predicate: () => boolean | Promise<boolean>, timeoutMs: number = 5_000) {
  const startTime = Date.now()

  while (Date.now() - startTime < timeoutMs) {
    if (await predicate()) {
      return
    }
    await sleep(25)
  }

  throw new Error("Timed out waiting for condition")
}

async function sleep(ms: number): Promise<void> {
  await new Promise((resolve) => setTimeout(resolve, ms))
}
