import * as assert from "node:assert/strict"
import * as fs from "node:fs/promises"
import * as os from "node:os"
import * as path from "node:path"
import { afterEach, test, vi } from "vitest"

const mockedExitHooks = vi.hoisted(() => ({
  callbacks: [] as Array<() => void>,
}))

const mockedLoop = vi.hoisted(() => {
  const session = {
    sessionId: "loop-session",
    stop: vi.fn(async () => {}),
  }

  return {
    session,
    runAgentLoop: vi.fn(async () => session),
  }
})

vi.mock("exit-hook", () => ({
  default: vi.fn((callback: () => void) => {
    mockedExitHooks.callbacks.push(callback)
    return () => {
      const callbackIndex = mockedExitHooks.callbacks.indexOf(callback)
      if (callbackIndex >= 0) {
        mockedExitHooks.callbacks.splice(callbackIndex, 1)
      }
    }
  }),
}))

vi.mock("../../src/loop/run-agent-loop.ts", () => ({
  runAgentLoop: mockedLoop.runAgentLoop,
}))

import { buildLoopParams, resolveLoop, runAdHocLoop } from "../../src/node/loops.ts"

const originalHome = process.env.HOME

afterEach(() => {
  mockedExitHooks.callbacks.length = 0
  mockedLoop.session.stop.mockClear()
  mockedLoop.runAgentLoop.mockClear()

  if (originalHome === undefined) {
    delete process.env.HOME
    return
  }

  process.env.HOME = originalHome
})

test("resolveLoop merges root defaults with packaged loop config", async () => {
  const tempDir = await fs.mkdtemp(path.join(os.tmpdir(), "goddard-loop-dir-"))
  const loopDir = path.join(tempDir, ".goddard", "loops", "review")

  await fs.mkdir(loopDir, { recursive: true })
  await fs.writeFile(
    path.join(tempDir, ".goddard", "config.json"),
    JSON.stringify({
      loops: {
        session: {
          agent: "pi-acp",
          cwd: "/tmp/root",
          mcpServers: [],
        },
        rateLimits: {
          cycleDelay: "1s",
          maxOpsPerMinute: 5,
          maxCyclesBeforePause: 10,
        },
      },
    }),
    "utf-8",
  )
  await fs.writeFile(
    path.join(loopDir, "config.json"),
    JSON.stringify({
      session: {
        systemPrompt: "Use the loop checklist.",
      },
      retries: {
        maxAttempts: 2,
      },
    }),
    "utf-8",
  )
  await fs.writeFile(
    path.join(loopDir, "prompt.js"),
    'export function nextPrompt() { return "Continue the current task." }\n',
    "utf-8",
  )

  const loop = await resolveLoop("review", tempDir)

  assert.equal(loop.promptModulePath, path.join(loopDir, "prompt.js"))
  assert.deepEqual(loop.config, {
    session: {
      agent: "pi-acp",
      cwd: "/tmp/root",
      mcpServers: [],
      systemPrompt: "Use the loop checklist.",
    },
    rateLimits: {
      cycleDelay: "1s",
      maxOpsPerMinute: 5,
      maxCyclesBeforePause: 10,
    },
    retries: {
      maxAttempts: 2,
    },
  })
})

test("buildLoopParams loads nextPrompt from prompt.js", async () => {
  const tempDir = await fs.mkdtemp(path.join(os.tmpdir(), "goddard-loop-build-"))
  const promptModulePath = path.join(tempDir, "prompt.js")

  await fs.writeFile(
    promptModulePath,
    'export function nextPrompt() { return "Continue the current task." }\n',
    "utf-8",
  )

  const params = await buildLoopParams({
    path: tempDir,
    promptModulePath,
    config: {
      session: {
        agent: "pi-acp",
        cwd: tempDir,
        mcpServers: [],
      },
      rateLimits: {
        cycleDelay: "1s",
        maxOpsPerMinute: 5,
        maxCyclesBeforePause: 10,
      },
    },
  })

  assert.equal(params.nextPrompt(), "Continue the current task.")
  assert.equal(params.retries.maxAttempts, 1)
})

test("resolveLoop rejects prompt.md-only loop packages", async () => {
  const tempDir = await fs.mkdtemp(path.join(os.tmpdir(), "goddard-loop-invalid-"))
  const loopDir = path.join(tempDir, ".goddard", "loops", "review")

  await fs.mkdir(loopDir, { recursive: true })
  await fs.writeFile(path.join(loopDir, "prompt.md"), "Legacy prompt.\n", "utf-8")
  await fs.writeFile(path.join(loopDir, "config.json"), JSON.stringify({}), "utf-8")

  await assert.rejects(resolveLoop("review", tempDir), /must not contain prompt\.md/)
})

test("runAdHocLoop imports promptModulePath and forwards resolved params", async () => {
  const tempDir = await fs.mkdtemp(path.join(os.tmpdir(), "goddard-loop-ad-hoc-"))
  const promptModulePath = path.join(tempDir, "prompt.js")

  await fs.writeFile(
    promptModulePath,
    'export function nextPrompt() { return "Continue the ad hoc task." }\n',
    "utf-8",
  )

  await runAdHocLoop({
    promptModulePath,
    session: {
      agent: "pi-acp",
      cwd: tempDir,
      mcpServers: [],
    },
    rateLimits: {
      cycleDelay: "1s",
      maxOpsPerMinute: 5,
      maxCyclesBeforePause: 10,
    },
  })

  assert.equal(mockedLoop.runAgentLoop.mock.calls.length, 1)
  const firstCall = mockedLoop.runAgentLoop.mock.calls[0]
  assert.ok(firstCall)
  const [params] = firstCall as unknown as [Awaited<ReturnType<typeof buildLoopParams>>]
  assert.equal(params.nextPrompt(), "Continue the ad hoc task.")
  assert.equal(params.retries.maxAttempts, 1)
  assert.equal(mockedExitHooks.callbacks.length, 1)

  await mockedExitHooks.callbacks[0]()

  assert.equal(mockedLoop.session.stop.mock.calls.length, 1)
})
