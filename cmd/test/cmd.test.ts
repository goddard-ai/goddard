import { test } from "vitest"
import * as assert from "node:assert/strict"
import { runCli, type CliIo, type CliDeps } from "../src/index.ts"
import { createSdk, PROPOSE_SYSTEM_PROMPT } from "@goddard-ai/sdk"
import { Models } from "@goddard-ai/config"

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
    createSdkClient: () => sdk,
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
  assert.equal(spawnCalls[0]!.args[1], PROPOSE_SYSTEM_PROMPT)
  assert.equal(spawnCalls[0]!.args[2], "add auth")
})

type SdkClient = ReturnType<typeof createSdk>

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
      models: Models,
    },
  } as unknown as SdkClient
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
    createSdkClient: () => sdk,
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

  const exitCode = await runCli(["loop", "init"], io, { createSdkClient: () => sdk })
  assert.equal(exitCode, 0)
  assert.ok(lines.some((l) => l.includes("Created configuration at /mock/config.ts")))
})
