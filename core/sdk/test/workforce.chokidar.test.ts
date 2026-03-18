import * as assert from "node:assert/strict"
import * as fs from "node:fs/promises"
import * as os from "node:os"
import * as path from "node:path"
import { afterEach, beforeEach, test, vi } from "vitest"

const { promptMock, stopMock, runAgentMock, watcherCloseMock, watcherOnMock, watchMock } =
  vi.hoisted(() => {
    const prompt = vi.fn(async () => {})
    const stop = vi.fn(async () => {})
    const runAgent = vi.fn(async ({ cwd }: { cwd: string }) => ({
      sessionId: `session:${cwd}`,
      prompt,
      stop,
    }))
    const watcherClose = vi.fn(async () => {})
    let watcher: { on: (...args: unknown[]) => unknown; close: () => Promise<void> }
    const watcherOn = vi.fn(() => watcher)
    watcher = {
      on: watcherOn,
      close: watcherClose,
    }
    const watch = vi.fn((_directory: string, _options?: unknown) => watcher)

    return {
      promptMock: prompt,
      stopMock: stop,
      runAgentMock: runAgent,
      watcherCloseMock: watcherClose,
      watcherOnMock: watcherOn,
      watchMock: watch,
    }
  })

vi.mock("../src/daemon/session/client.ts", () => ({
  runAgent: runAgentMock,
}))

vi.mock("chokidar", () => ({
  watch: watchMock,
}))

const supervisors: Array<{ stop: () => Promise<void> }> = []

beforeEach(() => {
  promptMock.mockReset()
  stopMock.mockReset()
  runAgentMock.mockClear()
  watchMock.mockClear()
  watcherOnMock.mockClear()
  watcherCloseMock.mockClear()
})

afterEach(async () => {
  while (supervisors.length > 0) {
    await supervisors.pop()?.stop()
  }
})

test("watchWorkforce uses chokidar to monitor package .goddard directories", async () => {
  const rootDir = await fs.mkdtemp(path.join(os.tmpdir(), "goddard-workforce-chokidar-"))
  const goddardDir = path.join(rootDir, ".goddard")

  await fs.mkdir(goddardDir, { recursive: true })
  await fs.writeFile(
    path.join(rootDir, "package.json"),
    JSON.stringify({ name: "@repo/root" }),
    "utf-8",
  )
  await fs.writeFile(path.join(goddardDir, "requests.jsonl"), "", "utf-8")
  await fs.writeFile(path.join(goddardDir, "responses.jsonl"), "", "utf-8")

  const { watchWorkforce } = await import("../src/node/workforce.ts")
  const supervisor = await watchWorkforce({ rootDir })
  supervisors.push(supervisor)

  assert.equal(watchMock.mock.calls.length, 1)
  assert.equal(watchMock.mock.calls[0][0], goddardDir)
})

test("WorkforceSupervisor.stop closes chokidar watchers", async () => {
  const rootDir = await fs.mkdtemp(path.join(os.tmpdir(), "goddard-workforce-chokidar-stop-"))
  const goddardDir = path.join(rootDir, ".goddard")

  await fs.mkdir(goddardDir, { recursive: true })
  await fs.writeFile(
    path.join(rootDir, "package.json"),
    JSON.stringify({ name: "@repo/root" }),
    "utf-8",
  )
  await fs.writeFile(path.join(goddardDir, "requests.jsonl"), "", "utf-8")
  await fs.writeFile(path.join(goddardDir, "responses.jsonl"), "", "utf-8")

  const { watchWorkforce } = await import("../src/node/workforce.ts")
  const supervisor = await watchWorkforce({ rootDir })

  await supervisor.stop()

  assert.equal(watcherCloseMock.mock.calls.length, 1)
})
