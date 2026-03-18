import { afterEach, test, vi } from "vitest"
import * as assert from "node:assert/strict"
import { lstat, mkdtemp, mkdir, rm, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { spawnSync } from "node:child_process"

import { runDaemon, type RunDaemonDeps } from "../src/daemon.ts"
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

  on(eventName: string, handler: (payload?: unknown) => void): this {
    const handlers = this.#handlers.get(eventName) ?? []
    handlers.push(handler)
    this.#handlers.set(eventName, handlers)
    return this
  }

  close(): void {
    // no-op for tests
  }

  emit(eventName: string, payload: unknown): void {
    for (const handler of this.#handlers.get(eventName) ?? []) {
      void handler(payload)
    }
  }
}

test("daemon package ships a goddard wrapper in agent-bin", async () => {
  const wrapperPath = new URL("../agent-bin/goddard", import.meta.url)
  const stat = await lstat(wrapperPath)
  assert.equal(stat.isSymbolicLink() || stat.isFile(), true)
})

test("daemon run subscribes to repo, starts IPC, and passes daemon URL into one-shot runs", async () => {
  const subscription = new MockStreamSubscription()
  let subCalls = 0

  const runOneShotCalls: any[] = []
  const startIpcCalls: any[] = []
  const deps: RunDaemonDeps = {
    createBackendClient: async () => ({
      pr: {
        create: async () => ({
          number: 1,
          url: "https://github.com/acme/widgets/pull/1",
        }),
        reply: async () => ({ success: true }),
        isManaged: async () => true,
      },
      stream: {
        subscribeToRepo: async () => {
          subCalls += 1
          return subscription
        },
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
    runOneShot: async (input) => {
      runOneShotCalls.push(input)
      return 0
    },
    waitForShutdown: async (close) => {
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

  const { logs, result: exitCode } = await captureDaemonLogs(async () =>
    runDaemon(
      {
        repo: "test/repo",
        projectDir: process.cwd(),
        baseUrl: "",
        socketPath: "/tmp/custom-daemon.sock",
        agentBinDir: "/tmp/custom-agent-bin",
      },
      deps,
    ),
  )

  assert.equal(exitCode, 0)
  assert.equal(subCalls, 1)
  assert.deepEqual(startIpcCalls, [
    {
      socketPath: "/tmp/custom-daemon.sock",
      agentBinDir: "/tmp/custom-agent-bin",
    },
  ])
  assert.equal(runOneShotCalls.length, 1)
  assert.equal(runOneShotCalls[0].event.prNumber, 123)
  assert.equal(
    runOneShotCalls[0].daemonUrl,
    "http://unix/?socketPath=%2Ftmp%2Fgoddard-daemon-test.sock",
  )
  assert.equal(runOneShotCalls[0].agentBinDir, "/tmp/custom-agent-bin")
  assert.match(runOneShotCalls[0].prompt, /goddard reply-pr --message-file/)
  assert.doesNotMatch(runOneShotCalls[0].prompt, /goddard pr reply --body/)

  const startupLog = logs.find((entry) => entry.event === "daemon.startup")
  assert.deepEqual(startupLog, {
    scope: "daemon",
    at: startupLog?.at,
    event: "daemon.startup",
    repository: "test/repo",
    projectDir: process.cwd(),
    baseUrl: "http://127.0.0.1:8787",
    socketPath: "/tmp/custom-daemon.sock",
    agentBinDir: "/tmp/custom-agent-bin",
  })
  assert.equal(
    logs.some((entry) => entry.event === "repo.subscription_started"),
    true,
  )
  assert.equal(
    logs.some((entry) => entry.event === "one_shot.launch"),
    true,
  )
  assert.equal(
    logs.some((entry) => entry.event === "one_shot.finish"),
    true,
  )
  assert.equal(
    logs.some((entry) => entry.event === "daemon.shutdown"),
    true,
  )
})

test("daemon URL round-trips the socket path", () => {
  const socketPath = "/tmp/goddard-daemon.sock"
  const daemonUrl = createDaemonUrl(socketPath)

  assert.equal(daemonUrl, "http://unix/?socketPath=%2Ftmp%2Fgoddard-daemon.sock")
  assert.equal(readSocketPathFromDaemonUrl(daemonUrl), socketPath)
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
  assert.deepEqual(submit, {
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
  assert.deepEqual(reply, {
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
  assert.equal(result.status, 0, result.stderr)
}

async function captureDaemonLogs<T>(
  action: () => Promise<T>,
): Promise<{ logs: Array<Record<string, unknown>>; result: T }> {
  const output: string[] = []
  const stdout = vi.spyOn(process.stdout, "write").mockImplementation(((
    chunk: string | Uint8Array,
  ) => {
    output.push(typeof chunk === "string" ? chunk : Buffer.from(chunk).toString("utf-8"))
    return true
  }) as typeof process.stdout.write)

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
    stdout.mockRestore()
  }
}
