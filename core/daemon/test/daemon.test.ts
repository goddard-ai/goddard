import { spawnSync } from "node:child_process"
import { lstat, mkdir, mkdtemp, rm, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { afterEach, expect, test } from "bun:test"

import type { BackendClient } from "../src/backend.ts"
import { runDaemon, type RunDeps } from "../src/daemon.ts"
import {
  createDaemonUrl,
  readSocketPathFromDaemonUrl,
  resolveReplyRequestFromGit,
  resolveSubmitRequestFromGit,
} from "../src/ipc.ts"

const cleanup: Array<() => Promise<void>> = []

afterEach(async () => {
  while (cleanup.length > 0) {
    await cleanup.pop()?.()
  }
})

class MockStreamSubscription {
  #handlers = new Map<string, ((payload?: unknown) => void)[]>()
  #closed = false

  on(eventName: string, handler: (payload?: unknown) => void): this {
    const handlers = this.#handlers.get(eventName) ?? []
    handlers.push(handler)
    this.#handlers.set(eventName, handlers)
    return this
  }

  off(eventName: string, handler: (payload?: unknown) => void): this {
    const handlers = this.#handlers.get(eventName) ?? []
    this.#handlers.set(
      eventName,
      handlers.filter((candidate) => candidate !== handler),
    )
    return this
  }

  close(): void {
    this.#closed = true
  }

  emit(eventName: string, payload: unknown): void {
    for (const handler of this.#handlers.get(eventName) ?? []) {
      void handler(payload)
    }
  }

  isClosed(): boolean {
    return this.#closed
  }
}

function createMockBackendClient(
  input: {
    subscription?: MockStreamSubscription
    onSubscribe?: () => void
  } = {},
): BackendClient {
  return {
    auth: {
      startDeviceFlow: async () => ({
        deviceCode: "dev",
        userCode: "code",
        verificationUri: "https://github.com/login/device",
        expiresIn: 900,
        interval: 5,
      }),
      completeDeviceFlow: async () => ({
        token: "tok",
        githubUsername: "alec",
        githubUserId: 1,
      }),
      whoami: async () => ({
        token: "tok",
        githubUsername: "alec",
        githubUserId: 1,
      }),
      logout: async () => {},
    },
    pr: {
      create: async (request) => ({
        id: 1,
        number: 1,
        owner: request.owner,
        repo: request.repo,
        title: request.title,
        body: request.body ?? "",
        head: request.head,
        base: request.base,
        url: "https://github.com/acme/widgets/pull/1",
        createdBy: "alec",
        createdAt: "2026-01-01T00:00:00.000Z",
      }),
      reply: async () => ({ success: true }),
      isManaged: async () => true,
    },
    stream: {
      subscribe: async () => {
        input.onSubscribe?.()
        return input.subscription ?? new MockStreamSubscription()
      },
    },
  }
}

test("daemon package ships agent-bin wrappers for goddard and workforce", async () => {
  const wrapperPath = new URL("../agent-bin/goddard", import.meta.url)
  const workforceWrapperPath = new URL("../agent-bin/workforce", import.meta.url)
  const [goddardStat, workforceStat] = await Promise.all([
    lstat(wrapperPath),
    lstat(workforceWrapperPath),
  ])
  expect(goddardStat.isSymbolicLink() || goddardStat.isFile()).toBe(true)
  expect(workforceStat.isSymbolicLink() || workforceStat.isFile()).toBe(true)
})

test("daemon run subscribes once, handles events across repositories, and passes daemon URL into the PR feedback flow", async () => {
  const subscription = new MockStreamSubscription()
  let subCalls = 0

  const runPrFeedbackFlowCalls: any[] = []
  const startIpcCalls: any[] = []
  const deps: RunDeps = {
    createBackendClient: async () =>
      createMockBackendClient({
        subscription,
        onSubscribe: () => {
          subCalls += 1
        },
      }),
    startIpcServer: async (_client, options) => {
      startIpcCalls.push(options)
      return {
        daemonUrl: "http://unix/?socketPath=%2Ftmp%2Fgoddard-daemon-test.sock",
        socketPath: "/tmp/goddard-daemon-test.sock",
        close: async () => {},
      }
    },
    runPrFeedbackFlow: async (input) => {
      runPrFeedbackFlowCalls.push(input)
      return 0
    },
    waitForShutdown: async (close) => {
      subscription.emit("event", {
        type: "comment" as const,
        owner: "other",
        repo: "repo",
        prNumber: 123,
        author: "alice",
        body: "handle this too",
        reactionAdded: "eyes",
        createdAt: new Date().toISOString(),
      })
      const event = {
        type: "comment" as const,
        owner: "test",
        repo: "repo",
        prNumber: 123,
        author: "alice",
        body: "fix it",
        reactionAdded: "eyes",
        createdAt: new Date().toISOString(),
      }
      subscription.emit("event", event)
      await new Promise((resolve) => setTimeout(resolve, 0))
      await close()
    },
  }

  const { logs, result: exitCode } = await captureLogs((io) =>
    runDaemon(
      {
        baseUrl: "",
        socketPath: "/tmp/custom-daemon.sock",
        agentBinDir: "/tmp/custom-agent-bin",
        logMode: "json",
      },
      {
        ...deps,
        io,
      },
    ),
  )

  expect(exitCode).toBe(0)
  expect(subCalls).toBe(1)
  expect(startIpcCalls).toEqual([
    {
      socketPath: "/tmp/custom-daemon.sock",
      agentBinDir: "/tmp/custom-agent-bin",
    },
  ])
  expect(runPrFeedbackFlowCalls).toHaveLength(2)
  expect(
    runPrFeedbackFlowCalls.map(
      (call) => `${call.event.owner}/${call.event.repo}#${call.event.prNumber}`,
    ),
  ).toEqual(["other/repo#123", "test/repo#123"])
  expect(runPrFeedbackFlowCalls[0].event.prNumber).toBe(123)
  expect(runPrFeedbackFlowCalls[0].daemonUrl).toBe(
    "http://unix/?socketPath=%2Ftmp%2Fgoddard-daemon-test.sock",
  )
  expect(runPrFeedbackFlowCalls[0].agentBinDir).toBe("/tmp/custom-agent-bin")
  expect(runPrFeedbackFlowCalls[0].prompt).toMatch(/goddard reply-pr --message-file/)
  expect(runPrFeedbackFlowCalls[0].prompt).not.toMatch(/goddard pr reply --body/)

  const startupLog = logs.find((entry) => entry.event === "daemon.startup")
  expect(startupLog).toEqual({
    scope: "daemon",
    at: startupLog?.at,
    event: "daemon.startup",
    baseUrl: "http://127.0.0.1:8787",
    socketPath: "/tmp/custom-daemon.sock",
    agentBinDir: "/tmp/custom-agent-bin",
  })
  expect(logs.some((entry) => entry.event === "repo.subscription_started")).toBe(true)
  expect(
    logs.some(
      (entry) =>
        entry.event === "pr_feedback.launch" &&
        typeof entry.feedbackEvent === "object" &&
        entry.feedbackEvent !== null &&
        (entry.feedbackEvent as Record<string, unknown>).repository === "test/repo" &&
        (entry.feedbackEvent as Record<string, unknown>).prNumber === 123 &&
        (entry.feedbackEvent as Record<string, unknown>).feedbackType === "comment",
    ),
  ).toBe(true)
  expect(
    logs.some(
      (entry) =>
        entry.event === "pr_feedback.finish" &&
        typeof entry.feedbackEvent === "object" &&
        entry.feedbackEvent !== null &&
        (entry.feedbackEvent as Record<string, unknown>).repository === "test/repo" &&
        (entry.feedbackEvent as Record<string, unknown>).prNumber === 123 &&
        (entry.feedbackEvent as Record<string, unknown>).feedbackType === "comment",
    ),
  ).toBe(true)
  expect(logs.some((entry) => entry.event === "daemon.shutdown")).toBe(true)
})

test("daemon run can start only the IPC server when stream is disabled", async () => {
  let subCalls = 0
  const startIpcCalls: Array<{ socketPath: string; agentBinDir: string }> = []

  const { logs, result: exitCode } = await captureLogs((io) =>
    runDaemon(
      {
        baseUrl: "",
        socketPath: "/tmp/ipc-only.sock",
        agentBinDir: "/tmp/custom-agent-bin",
        enableIpc: true,
        enableStream: false,
        logMode: "json",
      },
      {
        io,
        createBackendClient: async () =>
          createMockBackendClient({
            onSubscribe: () => {
              subCalls += 1
            },
          }),
        startIpcServer: async (_client, options) => {
          startIpcCalls.push(options)
          return {
            daemonUrl: "http://unix/?socketPath=%2Ftmp%2Fipc-only.sock",
            socketPath: "/tmp/ipc-only.sock",
            close: async () => {},
          }
        },
        waitForShutdown: async (close) => {
          await close()
        },
      },
    ),
  )

  expect(exitCode).toBe(0)
  expect(subCalls).toBe(0)
  expect(startIpcCalls).toEqual([
    {
      socketPath: "/tmp/ipc-only.sock",
      agentBinDir: "/tmp/custom-agent-bin",
    },
  ])
  expect(logs.some((entry) => entry.event === "repo.subscription_started")).toBe(false)
  expect(logs.some((entry) => entry.event === "daemon.shutdown")).toBe(true)
})

test("daemon run can subscribe without IPC and ignores feedback that requires the PR feedback flow", async () => {
  const subscription = new MockStreamSubscription()
  let subCalls = 0
  const runPrFeedbackFlowCalls: any[] = []
  const startIpcCalls: any[] = []

  const { logs, result: exitCode } = await captureLogs((io) =>
    runDaemon(
      {
        baseUrl: "",
        enableIpc: false,
        enableStream: true,
        logMode: "json",
      },
      {
        io,
        createBackendClient: async () =>
          createMockBackendClient({
            subscription,
            onSubscribe: () => {
              subCalls += 1
            },
          }),
        startIpcServer: async (_client, options) => {
          startIpcCalls.push(options)
          return {
            daemonUrl: "http://unix/?socketPath=%2Ftmp%2Fnot-used.sock",
            socketPath: "/tmp/not-used.sock",
            close: async () => {},
          }
        },
        runPrFeedbackFlow: async (input) => {
          runPrFeedbackFlowCalls.push(input)
          return 0
        },
        waitForShutdown: async (close) => {
          subscription.emit("event", {
            type: "comment" as const,
            owner: "test",
            repo: "repo",
            prNumber: 456,
            author: "alice",
            body: "fix it",
            reactionAdded: "eyes",
            createdAt: new Date().toISOString(),
          })
          await new Promise((resolve) => setTimeout(resolve, 0))
          await close()
        },
      },
    ),
  )

  expect(exitCode).toBe(0)
  expect(subCalls).toBe(1)
  expect(startIpcCalls).toEqual([])
  expect(runPrFeedbackFlowCalls).toEqual([])
  expect(
    logs.some(
      (entry) => entry.event === "repo.feedback_ignored" && entry.reason === "ipc_disabled",
    ),
  ).toBe(true)
})

test("daemon run defaults to concise pretty terminal logs", async () => {
  const lines: string[] = []

  const exitCode = await runDaemon(
    {
      baseUrl: "",
      enableIpc: false,
      enableStream: false,
    },
    {
      io: {
        stdout(line) {
          lines.push(line)
        },
        stderr() {},
      },
    },
  )

  expect(exitCode).toBe(0)
  expect(lines.some((line) => line.includes("daemon.startup"))).toBe(true)
  expect(lines.some((line) => line.includes("daemon.no_features_enabled"))).toBe(true)
  expect(lines.every((line) => line.trim().startsWith("{"))).toBe(false)
})

test("daemon run supports raw json terminal logs when requested", async () => {
  const lines: string[] = []

  const exitCode = await runDaemon(
    {
      baseUrl: "",
      enableIpc: false,
      enableStream: false,
      logMode: "json",
    },
    {
      io: {
        stdout(line) {
          lines.push(line)
        },
        stderr() {},
      },
    },
  )

  expect(exitCode).toBe(0)
  expect(lines.some((line) => line.includes('"event":"daemon.startup"'))).toBe(true)
  expect(lines.some((line) => line.includes('"event":"daemon.no_features_enabled"'))).toBe(true)
  expect(lines.every((line) => line.trim().startsWith("{"))).toBe(true)
})

test("daemon run supports verbose terminal logs with expanded fields", async () => {
  const lines: string[] = []

  const exitCode = await runDaemon(
    {
      baseUrl: "",
      enableIpc: false,
      enableStream: false,
      logMode: "verbose",
    },
    {
      io: {
        stdout(line) {
          lines.push(line)
        },
        stderr() {},
      },
    },
  )

  expect(exitCode).toBe(0)
  expect(lines.some((line) => line.includes("daemon.startup"))).toBe(true)
  expect(lines.some((line) => line.includes("baseUrl:"))).toBe(true)
  expect(lines.every((line) => line.trim().startsWith("{"))).toBe(false)
})

test("daemon URL round-trips the socket path", () => {
  const socketPath = "/tmp/goddard-daemon.sock"
  const daemonUrl = createDaemonUrl(socketPath)

  expect(daemonUrl).toBe("http://unix/?socketPath=%2Ftmp%2Fgoddard-daemon.sock")
  expect(readSocketPathFromDaemonUrl(daemonUrl)).toBe(socketPath)
})

test("daemon resolves PR context from git metadata", async () => {
  const repoDir = await mkdtemp(join(tmpdir(), "goddard-daemon-git-"))
  cleanup.push(async () => {
    await rm(repoDir, { recursive: true, force: true })
  })

  runGit(repoDir, ["init"])
  runGit(repoDir, ["config", "user.name", "Goddard"])
  runGit(repoDir, ["config", "user.email", "goddard@example.com"])
  await writeFile(join(repoDir, "README.md"), "# test\n", "utf-8")
  runGit(repoDir, ["add", "README.md"])
  runGit(repoDir, ["commit", "-m", "init"])
  runGit(repoDir, ["checkout", "-b", "feature/ipc"])
  runGit(repoDir, ["remote", "add", "origin", "git@github.com:acme/widgets.git"])
  await mkdir(join(repoDir, ".git", "refs", "remotes", "origin"), { recursive: true })
  await writeFile(
    join(repoDir, ".git", "refs", "remotes", "origin", "HEAD"),
    "ref: refs/remotes/origin/main\n",
  )

  const submit = await resolveSubmitRequestFromGit({
    cwd: repoDir,
    title: "Implement IPC routing",
    body: "Done.",
  })
  expect(submit).toEqual({
    owner: "acme",
    repo: "widgets",
    title: "Implement IPC routing",
    body: "Done.",
    head: "feature/ipc",
    base: "main",
  })

  runGit(repoDir, ["checkout", "-B", "pr-12"])
  const reply = await resolveReplyRequestFromGit({
    cwd: repoDir,
    message: "Updated per review",
  })
  expect(reply).toEqual({
    owner: "acme",
    repo: "widgets",
    prNumber: 12,
    body: "Updated per review",
  })
})

function runGit(cwd: string, args: string[]): void {
  const result = spawnSync("git", args, {
    cwd,
    encoding: "utf-8",
  })
  expect(result.status).toBe(0)
}

async function captureLogs<T>(
  action: (io: { stdout: (line: string) => void; stderr: (line: string) => void }) => Promise<T>,
): Promise<{ logs: Array<Record<string, unknown>>; result: T }> {
  const output: string[] = []
  const io = {
    stdout: (line: string) => {
      output.push(line)
    },
    stderr: () => {},
  }

  const result = await action(io)
  return {
    logs: output
      .flatMap((chunk) => chunk.split("\n"))
      .filter((line) => line.trim().length > 0)
      .map((line) => JSON.parse(line) as Record<string, unknown>),
    result,
  }
}
