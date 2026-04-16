import { afterEach, expect, test } from "bun:test"
import { mkdtemp, rm, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join } from "node:path"

import { configureLogging } from "../src/logging.ts"
import { createLoopManager } from "../src/loop/manager.ts"
import { normalizeLoopRootDir } from "../src/loop/paths.ts"
import { LoopRuntime } from "../src/loop/runtime.ts"

const cleanup: Array<() => Promise<void>> = []

afterEach(async () => {
  while (cleanup.length > 0) {
    await cleanup.pop()?.()
  }
})

/** Polls until the assertion passes or the timeout expires under Bun test. */
async function waitForExpectation(
  assertion: () => void,
  {
    timeoutMs = 2_000,
    intervalMs = 25,
  }: {
    timeoutMs?: number
    intervalMs?: number
  } = {},
) {
  const deadline = Date.now() + timeoutMs

  while (true) {
    try {
      assertion()
      return
    } catch (error) {
      if (Date.now() >= deadline) {
        throw error
      }
    }

    await Bun.sleep(intervalMs)
  }
}

test("loop manager reuses one runtime per normalized repository root and loop name", async () => {
  const created: Array<{ rootDir: string; loopName: string }> = []
  const manager = createLoopManager({
    sessionManager: {} as never,
    resolveLoopStartRequest: async (input) => ({
      rootDir: input.rootDir,
      loopName: input.loopName,
      promptModulePath: `${input.rootDir}/.goddard/loops/${input.loopName}/prompt.js`,
      session: {
        agent: "pi-acp",
        cwd: input.rootDir,
        mcpServers: [],
        systemPrompt: "test",
      },
      rateLimits: {
        cycleDelay: "30s",
        maxOpsPerMinute: 4,
        maxCyclesBeforePause: 200,
      },
      retries: {
        maxAttempts: 1,
        initialDelayMs: 500,
        maxDelayMs: 5_000,
        backoffFactor: 2,
        jitterRatio: 0.2,
      },
    }),
    createRuntime: async (input) => {
      created.push({ rootDir: input.rootDir, loopName: input.loopName })
      return {
        getLoop: () => ({
          state: "running",
          rootDir: input.rootDir,
          loopName: input.loopName,
          promptModulePath: input.promptModulePath,
          startedAt: "2026-03-20T00:00:00.000Z",
          sessionId: "session-1",
          acpSessionId: "acp-1",
          cycleCount: 0,
          lastPromptAt: null,
          session: input.session,
          rateLimits: input.rateLimits,
          retries: input.retries,
        }),
        getStatus: () => ({
          state: "running",
          rootDir: input.rootDir,
          loopName: input.loopName,
          promptModulePath: input.promptModulePath,
          startedAt: "2026-03-20T00:00:00.000Z",
          sessionId: "session-1",
          acpSessionId: "acp-1",
          cycleCount: 0,
          lastPromptAt: null,
        }),
        stop: async () => {},
      } as unknown as LoopRuntime
    },
  })

  const tempRoot = await mkdtemp(join(tmpdir(), "goddard-loop-manager-"))
  cleanup.push(() => rm(tempRoot, { recursive: true, force: true }))

  await manager.startLoop({
    rootDir: tempRoot,
    loopName: "review",
    session: {
      agent: "pi-acp",
      cwd: tempRoot,
      mcpServers: [],
      systemPrompt: "test",
    },
    rateLimits: {
      cycleDelay: "30s",
      maxOpsPerMinute: 4,
      maxCyclesBeforePause: 200,
    },
    retries: {
      maxAttempts: 1,
      initialDelayMs: 500,
      maxDelayMs: 5_000,
      backoffFactor: 2,
      jitterRatio: 0.2,
    },
  })
  await manager.startLoop({
    rootDir: tempRoot,
    loopName: "review",
    session: {
      agent: "pi-acp",
      cwd: tempRoot,
      mcpServers: [],
      systemPrompt: "test",
    },
    rateLimits: {
      cycleDelay: "30s",
      maxOpsPerMinute: 4,
      maxCyclesBeforePause: 200,
    },
    retries: {
      maxAttempts: 1,
      initialDelayMs: 500,
      maxDelayMs: 5_000,
      backoffFactor: 2,
      jitterRatio: 0.2,
    },
  })

  expect(created).toEqual([
    {
      rootDir: await normalizeLoopRootDir(tempRoot),
      loopName: "review",
    },
  ])
})

test("loop runtime keeps prompting in the daemon-owned background and reports session status", async () => {
  const rootDir = await mkdtemp(join(tmpdir(), "goddard-loop-runtime-"))
  cleanup.push(() => rm(rootDir, { recursive: true, force: true }))
  await writeFile(
    join(rootDir, "prompt.js"),
    [
      "let cycle = 0",
      "export function nextPrompt() {",
      "  cycle += 1",
      "  return `cycle:${cycle}`",
      "}",
      "",
    ].join("\n"),
    "utf-8",
  )

  const newSessionCalls: unknown[] = []
  const promptSessionCalls: unknown[] = []
  const shutdownSessionCalls: unknown[] = []
  let promptResultIndex = 0
  const promptResults = [
    { stopReason: "max_tokens" },
    { stopReason: "max_tokens" },
    { stopReason: "end_turn" },
  ]

  const sessionManager = {
    async newSession(input: unknown) {
      newSessionCalls.push(input)
      return {
        id: "session-1",
        acpSessionId: "acp-1",
      }
    },
    async promptSession(...args: unknown[]) {
      promptSessionCalls.push(args)
      return promptResults[promptResultIndex++] ?? { stopReason: "end_turn" }
    },
    async shutdownSession(sessionId: string) {
      shutdownSessionCalls.push(sessionId)
      return true
    },
  }

  const { logs, result: runtime } = await captureLogs(async () => {
    const runtime = await LoopRuntime.start(
      {
        rootDir,
        loopName: "review",
        promptModulePath: join(rootDir, "prompt.js"),
        session: {
          agent: "pi-acp",
          cwd: rootDir,
          mcpServers: [],
          systemPrompt: "test",
        },
        rateLimits: {
          cycleDelay: "0s",
          maxOpsPerMinute: 100,
          maxCyclesBeforePause: 200,
        },
        retries: {
          maxAttempts: 1,
          initialDelayMs: 500,
          maxDelayMs: 5_000,
          backoffFactor: 2,
          jitterRatio: 0.2,
        },
      },
      {
        sessionManager: sessionManager as never,
      },
    )

    await waitForExpectation(() => {
      expect(promptSessionCalls).toHaveLength(3)
    })
    expect(newSessionCalls[0]).toEqual(
      expect.objectContaining({
        request: expect.objectContaining({
          cwd: rootDir,
          worktree: { enabled: true },
        }),
      }),
    )
    await waitForExpectation(() => {
      expect(shutdownSessionCalls).toContain("session-1")
    })

    return runtime
  })

  const startedLog = logs.find((entry) => entry.event === "loop.runtime_started")
  expect(startedLog).toBeTruthy()
  expect((startedLog?.loop as Record<string, unknown> | undefined)?.rootDir).toBe(rootDir)
  expect((startedLog?.loop as Record<string, unknown> | undefined)?.loopName).toBe("review")
  expect((startedLog?.loop as Record<string, unknown> | undefined)?.sessionId).toBe("session-1")
  expect((startedLog?.loop as Record<string, unknown> | undefined)?.acpSessionId).toBe("acp-1")

  const promptCompletedLog = logs.find((entry) => entry.event === "loop.prompt_completed")
  expect(promptCompletedLog).toBeTruthy()
  expect((promptCompletedLog?.loop as Record<string, unknown> | undefined)?.rootDir).toBe(rootDir)
  expect((promptCompletedLog?.loop as Record<string, unknown> | undefined)?.loopName).toBe("review")
  expect((promptCompletedLog?.loop as Record<string, unknown> | undefined)?.sessionId).toBe(
    "session-1",
  )
  expect((promptCompletedLog?.loop as Record<string, unknown> | undefined)?.acpSessionId).toBe(
    "acp-1",
  )

  const stoppedLog = logs.find((entry) => entry.event === "loop.runtime_stopped")
  expect(stoppedLog).toBeTruthy()
  expect((stoppedLog?.loop as Record<string, unknown> | undefined)?.rootDir).toBe(rootDir)
  expect((stoppedLog?.loop as Record<string, unknown> | undefined)?.loopName).toBe("review")
  expect((stoppedLog?.loop as Record<string, unknown> | undefined)?.sessionId).toBe("session-1")
  expect((stoppedLog?.loop as Record<string, unknown> | undefined)?.acpSessionId).toBe("acp-1")

  expect(runtime.getStatus()).toEqual(
    expect.objectContaining({
      rootDir,
      loopName: "review",
      sessionId: "session-1",
      acpSessionId: "acp-1",
      cycleCount: 3,
      lastPromptAt: expect.any(String),
    }),
  )
})

/** Captures daemon logs emitted while one loop runtime test action is running. */
async function captureLogs<T>(
  action: () => Promise<T>,
): Promise<{ logs: Array<Record<string, unknown>>; result: T }> {
  const output: string[] = []
  const restoreLogging = configureLogging({
    mode: "json",
    writeLine: (line) => {
      output.push(line)
    },
  })

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
    restoreLogging()
  }
}
