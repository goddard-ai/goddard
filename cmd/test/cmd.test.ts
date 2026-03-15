import { test, vi } from "vitest"
import * as assert from "node:assert/strict"

const { sdkCreateMock, mockedPrompts } = vi.hoisted(() => ({
  sdkCreateMock: vi.fn(),
  mockedPrompts: {
    PROPOSE_SYSTEM_PROMPT: "mock propose prompt",
    SPEC_SYSTEM_PROMPT: "mock spec prompt",
  },
}))

vi.mock("@goddard-ai/sdk", () => ({
  createSdk: sdkCreateMock,
  PROPOSE_SYSTEM_PROMPT: mockedPrompts.PROPOSE_SYSTEM_PROMPT,
  SPEC_SYSTEM_PROMPT: mockedPrompts.SPEC_SYSTEM_PROMPT,
}))

vi.mock("@goddard-ai/sdk/node", () => ({
  FileTokenStorage: class MockFileTokenStorage {},
}))

import { runCli, type CliIo, type CliDeps } from "../src/index.ts"

const defaultIo: CliIo = {
  stdout: () => {},
  stderr: () => {},
}

test("login command calls sdk.auth.login and prints username", async () => {
  const lines: string[] = []
  const io: CliIo = {
    stdout: (line) => lines.push(line),
    stderr: () => {},
  }

  const sdk = createMockSdk({
    auth: {
      login: async ({ githubUsername }) => ({
        token: "tok",
        githubUsername: githubUsername ?? "dev",
        githubUserId: 1,
      }),
    },
  })

  const exitCode = await runCli(["login", "--username", "testuser"], io, {
    createSdkClient: () => sdk as any,
  })

  assert.equal(exitCode, 0)
  assert.equal(lines.length, 1)
  assert.equal(lines[0], "Logged in as @testuser")
})

test("propose command spawns pi with correct arguments", async () => {
  const spawnCalls: { args: string[] }[] = []
  const deps: CliDeps = {
    spawnPi: (args) => {
      spawnCalls.push({ args })
      return 0
    },
  }

  const exitCode = await runCli(["propose", "add auth"], defaultIo, deps)

  assert.equal(exitCode, 0)
  assert.equal(spawnCalls.length, 1)
  assert.equal(spawnCalls[0]!.args[0], "--system-prompt")
  assert.equal(spawnCalls[0]!.args[1], mockedPrompts.PROPOSE_SYSTEM_PROMPT)
  assert.equal(spawnCalls[0]!.args[2], "add auth")
})

type SdkClient = {
  auth: {
    login: (input: {
      githubUsername?: string
      onPrompt: (verificationUri: string, userCode: string) => void
    }) => Promise<{ token: string; githubUsername: string; githubUserId: number }>
    startDeviceFlow: () => Promise<unknown>
    completeDeviceFlow: () => Promise<unknown>
    whoami: () => Promise<{ token: string; githubUsername: string; githubUserId: number }>
    logout: () => Promise<void>
  }
  pr: {
    create: (input: any) => Promise<{ number: number; url: string }>
    isManaged: (input: any) => Promise<boolean>
    reply: (input: any) => Promise<{ success: boolean }>
  }
  stream: {
    subscribeToRepo: (repo: { owner: string; repo: string }) => Promise<unknown>
  }
  agents: {
    init: (cwd?: string) => Promise<{ path: string }>
  }
  loop: {
    init: () => Promise<{ path: string }>
    run: () => Promise<void>
    generateSystemdService: () => Promise<{ path: string }>
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
      login: async () => ({ token: "tok", githubUsername: "dev", githubUserId: 1 }),
      startDeviceFlow: async () => ({
        deviceCode: "dev_default",
        userCode: "USER",
        verificationUri: "https://github.com/login/device",
        expiresIn: 900,
        interval: 5,
      }),
      completeDeviceFlow: async () => ({ token: "tok", githubUsername: "dev", githubUserId: 1 }),
      whoami: async () => ({ token: "tok", githubUsername: "dev", githubUserId: 1 }),
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
  }
}

test("agents init command calls sdk.agents.init and handles commit/push", async () => {
  const lines: string[] = []
  const execGitCalls: { cmd: string; args: string[] }[] = []

  const sdk = createMockSdk({
    agents: {
      init: async (cwd?: string) => ({ path: `${cwd ?? "mock"}/AGENTS.md` }),
    },
  })

  const deps: CliDeps = {
    createSdkClient: () => sdk as any,
    execGit: (cmd, args) => {
      execGitCalls.push({ cmd, args })
      if (cmd === "status") return { status: 0, stdout: "", stderr: "" }
      if (cmd === "diff") return { status: 0, stdout: "diff content", stderr: "" }
      return { status: 0, stdout: "", stderr: "" }
    },
    promptCommitMessage: async () => "init agents",
    promptPushBranch: async () => true,
  }

  const io: CliIo = {
    stdout: (line) => lines.push(line),
    stderr: () => {},
  }

  const exitCode = await runCli(["agents", "init"], io, deps)

  assert.equal(exitCode, 0)
  assert.ok(lines.some((l) => l.includes("Updated agents configuration")))
  assert.ok(execGitCalls.some((c) => c.cmd === "commit" && c.args.includes("init agents")))
  assert.ok(execGitCalls.some((c) => c.cmd === "push"))
})

test("loop init command calls sdk.loop.init", async () => {
  const lines: string[] = []
  const sdk = createMockSdk({
    loop: {
      init: async () => ({ path: "/mock/config.ts" }),
      run: async () => {},
      generateSystemdService: async () => ({ path: "" }),
    },
  })

  const io: CliIo = {
    stdout: (line) => lines.push(line),
    stderr: () => {},
  }

  const exitCode = await runCli(["loop", "init"], io, { createSdkClient: () => sdk as any })
  assert.equal(exitCode, 0)
  assert.ok(lines.some((l) => l.includes("Created configuration at /mock/config.ts")))
})

test("pr create routes through the daemon when a session token is present", async () => {
  const previousEnv = process.env
  process.env = {
    ...previousEnv,
    GODDARD_DAEMON_URL: "http://unix/?socketPath=%2Ftmp%2Fgoddard-daemon.sock",
    GODDARD_SESSION_TOKEN: "tok_session",
  }

  try {
    const lines: string[] = []
    const sdk = createMockSdk({
      pr: {
        create: async () => {
          throw new Error("sdk should not be used")
        },
      },
    })

    const daemonCalls: Array<Record<string, unknown>> = []
    const exitCode = await runCli(
      ["pr", "create", "--title", "Secure daemon routing", "--body", "Done."],
      {
        stdout: (line) => lines.push(line),
        stderr: () => {},
      },
      {
        createSdkClient: () => sdk as any,
        submitPrViaDaemon: async (input) => {
          daemonCalls.push(input)
          return {
            number: 12,
            url: "https://github.com/acme/widgets/pull/12",
          }
        },
      },
    )

    assert.equal(exitCode, 0)
    assert.deepEqual(daemonCalls, [
      {
        cwd: process.cwd(),
        title: "Secure daemon routing",
        body: "Done.",
        head: "main",
        base: "main",
      },
    ])
    assert.ok(
      lines.some((line) => line.includes("PR #12 created: https://github.com/acme/widgets/pull/12")),
    )
  } finally {
    process.env = previousEnv
  }
})

test("pr reply routes through the daemon when a session token is present", async () => {
  const previousEnv = process.env
  process.env = {
    ...previousEnv,
    GODDARD_DAEMON_URL: "http://unix/?socketPath=%2Ftmp%2Fgoddard-daemon.sock",
    GODDARD_SESSION_TOKEN: "tok_session",
  }

  try {
    const lines: string[] = []
    const sdk = createMockSdk({
      pr: {
        reply: async () => {
          throw new Error("sdk should not be used")
        },
      },
    })

    const daemonCalls: Array<Record<string, unknown>> = []
    const exitCode = await runCli(
      ["pr", "reply", "--body", "Updated per review", "--pr", "12"],
      {
        stdout: (line) => lines.push(line),
        stderr: () => {},
      },
      {
        createSdkClient: () => sdk as any,
        replyPrViaDaemon: async (input) => {
          daemonCalls.push(input)
          return { success: true }
        },
      },
    )

    assert.equal(exitCode, 0)
    assert.deepEqual(daemonCalls, [
      {
        cwd: process.cwd(),
        message: "Updated per review",
        prNumber: 12,
      },
    ])
    assert.ok(lines.some((line) => line.includes("Reply posted to PR #12")))
  } finally {
    process.env = previousEnv
  }
})
