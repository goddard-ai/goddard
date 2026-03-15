import { afterEach, test, vi } from "vitest"
import * as assert from "node:assert/strict"
import { mkdtemp, mkdir, rm, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { spawnSync } from "node:child_process"

const { createSessionPermissionsMock, revokeSessionPermissionsMock } = vi.hoisted(() => ({
  createSessionPermissionsMock: vi.fn(async () => undefined),
  revokeSessionPermissionsMock: vi.fn(async () => undefined),
}))

vi.mock("@goddard-ai/storage/session-permissions", () => ({
  SessionPermissionsStorage: {
    create: createSessionPermissionsMock,
    revoke: revokeSessionPermissionsMock,
  },
}))

import { runDaemon, type RunDaemonDeps } from "../src/daemon.ts"
import {
  createDaemonUrl,
  readSocketPathFromDaemonUrl,
  resolveReplyRequestFromGit,
  resolveSubmitRequestFromGit,
} from "../src/ipc.ts"

const cleanup: Array<() => Promise<void>> = []

afterEach(async () => {
  createSessionPermissionsMock.mockClear()
  revokeSessionPermissionsMock.mockClear()

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

test("daemon run subscribes to repo, starts IPC, and passes daemon context into one-shot runs", async () => {
  const subscription = new MockStreamSubscription()
  let subCalls = 0

  const runOneShotCalls: any[] = []
  const deps: RunDaemonDeps = {
    createSdkClient: async () => ({
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
    startIpcServer: async () => ({
      daemonUrl: "http://unix/?socketPath=%2Ftmp%2Fgoddard-daemon-test.sock",
      socketPath: "/tmp/goddard-daemon-test.sock",
      close: async () => {},
    }),
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

  const exitCode = await runDaemon(
    {
      repo: "test/repo",
      projectDir: process.cwd(),
      baseUrl: "",
    },
    deps,
  )

  assert.equal(exitCode, 0)
  assert.equal(subCalls, 1)
  assert.equal(runOneShotCalls.length, 1)
  assert.equal(runOneShotCalls[0].event.prNumber, 123)
  assert.equal(
    runOneShotCalls[0].env?.GODDARD_DAEMON_URL,
    "http://unix/?socketPath=%2Ftmp%2Fgoddard-daemon-test.sock",
  )
  assert.equal(typeof runOneShotCalls[0].env?.GODDARD_SESSION_TOKEN, "string")
  assert.match(runOneShotCalls[0].prompt, /goddard reply-pr --message-file/)
  assert.doesNotMatch(runOneShotCalls[0].prompt, /goddard pr reply --body/)
  assert.equal(createSessionPermissionsMock.mock.calls.length, 1)
  assert.equal(revokeSessionPermissionsMock.mock.calls.length, 1)
  assert.equal(
    createSessionPermissionsMock.mock.calls[0]?.[0]?.allowedPrNumbers?.[0],
    123,
  )
  assert.equal(
    createSessionPermissionsMock.mock.calls[0]?.[0]?.token,
    runOneShotCalls[0].env?.GODDARD_SESSION_TOKEN,
  )
  assert.equal(
    revokeSessionPermissionsMock.mock.calls[0]?.[0],
    createSessionPermissionsMock.mock.calls[0]?.[0]?.sessionId,
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
