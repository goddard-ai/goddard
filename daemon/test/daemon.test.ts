import { afterEach, test, vi } from "vitest"
import * as assert from "node:assert/strict"

vi.mock("@goddard-ai/storage/session-permissions", () => ({
  SessionPermissionsStorage: {
    create: vi.fn(async () => undefined),
    revoke: vi.fn(async () => undefined),
  },
}))

import { runDaemonCli, type DaemonIo, type DaemonDeps } from "../src/index.ts"
import { mkdtemp, mkdir, rm, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { spawnSync } from "node:child_process"
import {
  createDaemonUrl,
  readSocketPathFromDaemonUrl,
  resolveReplyRequestFromGit,
  resolveSubmitRequestFromGit,
} from "../src/ipc.ts"

const defaultIo: DaemonIo = {
  stdout: () => {},
  stderr: () => {},
}

const cleanup: Array<() => Promise<void>> = []

afterEach(async () => {
  while (cleanup.length > 0) {
    await cleanup.pop()?.()
  }
})

class MockStreamSubscription {
  #handlers = new Map<string, ((payload?: any) => void)[]>()

  on(eventName: string, handler: (payload?: any) => void): this {
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

type StreamSubscription = {
  on: (eventName: string, handler: (payload?: unknown) => void) => StreamSubscription
  close: () => void
}

type RepoEvent = {
  type: "comment" | "review" | "pr.created"
  owner: string
  repo: string
  prNumber: number
  author: string
  createdAt: string
  body?: string
  reactionAdded?: string
  state?: "approved" | "changes_requested" | "commented"
  title?: string
}

type SdkClient = {
  auth: {
    startDeviceFlow: () => Promise<{
      deviceCode: string
      userCode: string
      verificationUri: string
      expiresIn: number
      interval: number
    }>
    completeDeviceFlow: () => Promise<{ token: string; githubUsername: string; githubUserId: number }>
    whoami: () => Promise<{ token: string; githubUsername: string; githubUserId: number }>
    login: () => Promise<{ token: string; githubUsername: string; githubUserId: number }>
    logout: () => Promise<void>
  }
  pr: {
    create: (input: {
      owner: string
      repo: string
      title: string
      body?: string
      head: string
      base: string
    }) => Promise<{ number: number; url: string }>
    isManaged: (input: { owner: string; repo: string; prNumber: number }) => Promise<boolean>
    reply: (input: {
      owner: string
      repo: string
      prNumber: number
      body: string
    }) => Promise<{ success: boolean }>
  }
  stream: {
    subscribeToRepo: (repo: { owner: string; repo: string }) => Promise<StreamSubscription>
  }
  agents: {
    init: () => Promise<unknown>
  }
  loop: {
    init: () => Promise<unknown>
    run: () => Promise<void>
    generateSystemdService: () => Promise<unknown>
  }
  config: {
    models: Record<string, never>
  }
}

type PartialSdk = {
  auth?: Partial<SdkClient["auth"]>
  pr?: Partial<SdkClient["pr"]>
  stream?: Partial<SdkClient["stream"]>
  agents?: Partial<SdkClient["agents"]>
  loop?: Partial<SdkClient["loop"]>
}

function createMockSdk(partial: PartialSdk): SdkClient {
  return {
    auth: {
      startDeviceFlow: async () => ({
        deviceCode: "dev_default",
        userCode: "USER",
        verificationUri: "https://github.com/login/device",
        expiresIn: 900,
        interval: 5,
      }),
      completeDeviceFlow: async () => ({ token: "tok", githubUsername: "dev", githubUserId: 1 }),
      whoami: async () => ({ token: "tok", githubUsername: "dev", githubUserId: 1 }),
      login: async () => ({ token: "tok", githubUsername: "dev", githubUserId: 1 }),
      logout: async () => undefined,
      ...partial.auth,
    },
    pr: {
      create: async () => {
        throw new Error("not mocked")
      },
      isManaged: async () => false,
      reply: async () => ({ success: true }),
      ...partial.pr,
    },
    stream: {
      subscribeToRepo: async () => {
        throw new Error("not mocked")
      },
      ...partial.stream,
    },
    agents: {
      init: async () => {
        throw new Error("not mocked")
      },
      ...partial.agents,
    },
    loop: {
      init: async () => {
        throw new Error("not mocked")
      },
      run: async () => {
        throw new Error("not mocked")
      },
      generateSystemdService: async () => {
        throw new Error("not mocked")
      },
      ...partial.loop,
    },
    config: {
      models: {} as Record<string, never>,
    },
  } as unknown as SdkClient
}

test("daemon run command subscribes to repo and handles events", async () => {
  const subscription = new MockStreamSubscription()
  let subCalls = 0

  const sdk = createMockSdk({
    stream: {
      subscribeToRepo: async () => {
        subCalls++
        return subscription as unknown as StreamSubscription
      },
    },
    pr: {
      isManaged: async () => true,
    },
  })

  const runOneShotCalls: any[] = []
  const deps: DaemonDeps = {
    createSdkClient: () => sdk,
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
      // simulate an event
      const event: RepoEvent = {
        type: "comment",
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
      // then shut down
      await close()
    },
  }

  const exitCode = await runDaemonCli(["run", "--repo", "test/repo"], defaultIo, deps)
  assert.equal(exitCode, 0)
  assert.equal(subCalls, 1)
  assert.equal(runOneShotCalls.length, 1)
  assert.equal(runOneShotCalls[0].event.prNumber, 123)
  assert.match(runOneShotCalls[0].prompt, /goddard reply-pr --message-file/)
  assert.doesNotMatch(runOneShotCalls[0].prompt, /goddard pr reply --body/)
  assert.equal(
    runOneShotCalls[0].daemonUrl,
    "http://unix/?socketPath=%2Ftmp%2Fgoddard-daemon-test.sock",
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
  await writeFile(join(repoDir, ".git", "refs", "remotes", "origin", "HEAD"), "ref: refs/remotes/origin/main\n")

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
