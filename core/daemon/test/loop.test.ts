import { createDaemonIpcClient } from "@goddard-ai/daemon-client/node"
import { mkdtemp, rm, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { afterEach, expect, test } from "bun:test"
import { startDaemonServer } from "../src/ipc.ts"
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

test("daemon IPC exposes repo-root loop lifecycle methods", async () => {
  const socketDir = await mkdtemp(join(tmpdir(), "goddard-loop-ipc-"))
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
        whoami: async () => ({
          token: "tok_1",
          githubUsername: "alec",
          githubUserId: 42,
        }),
        logout: async () => {},
      },
      pr: {
        create: async () => ({ number: 1, url: "https://example.com/pr/1" }),
        reply: async () => ({ success: true }),
      },
    },
    {
      socketPath: join(socketDir, "daemon.sock"),
    },
    {
      createLoopManager: () => ({
        startLoop: async (input) => ({
          state: "running",
          rootDir: input.rootDir,
          loopName: input.loopName,
          promptModulePath: `${input.rootDir}/.goddard/loops/${input.loopName}/prompt.js`,
          startedAt: "2026-03-20T00:00:00.000Z",
          sessionId: "session-1",
          acpId: "acp-1",
          cycleCount: 0,
          lastPromptAt: null,
          session: input.session,
          rateLimits: input.rateLimits,
          retries: input.retries,
        }),
        getLoop: async (rootDir: string, loopName: string) => ({
          state: "running",
          rootDir,
          loopName,
          promptModulePath: `${rootDir}/.goddard/loops/${loopName}/prompt.js`,
          startedAt: "2026-03-20T00:00:00.000Z",
          sessionId: "session-1",
          acpId: "acp-1",
          cycleCount: 1,
          lastPromptAt: null,
          session: {
            agent: "pi-acp",
            cwd: rootDir,
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
        listLoops: async () => [
          {
            state: "running",
            rootDir: "/repo",
            loopName: "review",
            promptModulePath: "/repo/.goddard/loops/review/prompt.js",
            startedAt: "2026-03-20T00:00:00.000Z",
            sessionId: "session-1",
            acpId: "acp-1",
            cycleCount: 1,
            lastPromptAt: null,
          },
        ],
        shutdownLoop: async () => true,
        close: async () => {},
      }),
    },
  )
  cleanup.push(async () => {
    await daemon.close()
    await rm(socketDir, { recursive: true, force: true })
  })

  const client = createDaemonIpcClient({ daemonUrl: daemon.daemonUrl })
  const started = await client.send("loopStart", {
    rootDir: "/repo",
    loopName: "review",
    session: {
      agent: "pi-acp",
      cwd: "/repo",
      mcpServers: [],
      systemPrompt: "Use the loop checklist.",
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
  const fetched = await client.send("loopGet", { rootDir: "/repo", loopName: "review" })
  const listed = await client.send("loopList", {})
  const stopped = await client.send("loopShutdown", { rootDir: "/repo", loopName: "review" })

  expect(started.loop.loopName).toBe("review")
  expect(fetched.loop.rootDir).toBe("/repo")
  expect(listed.loops).toHaveLength(1)
  expect(stopped.success).toBe(true)
})

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
          acpId: "acp-1",
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
          acpId: "acp-1",
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
        acpId: "acp-1",
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
      cwd: rootDir,
      worktree: { enabled: true },
    }),
  )
  await waitForExpectation(() => {
    expect(shutdownSessionCalls).toContain("session-1")
  })

  expect(runtime.getStatus()).toEqual(
    expect.objectContaining({
      rootDir,
      loopName: "review",
      sessionId: "session-1",
      acpId: "acp-1",
      cycleCount: 3,
      lastPromptAt: expect.any(String),
    }),
  )
})
