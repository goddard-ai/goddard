import { spawnSync } from "node:child_process"
import { mkdtemp, rm, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { createDaemonIpcClient } from "@goddard-ai/daemon-client/node"
import type { DaemonSession } from "@goddard-ai/schema/daemon"
import { afterAll, afterEach, expect, test } from "bun:test"

import { startDaemonServer, type DaemonServer } from "../src/ipc.ts"
import type { BackendPrClient } from "../src/ipc/types.ts"
import { configureLogging } from "../src/logging.ts"
import { db, resetDb } from "../src/persistence/store.ts"

const cleanup: Array<() => Promise<void>> = []
const originalHome = process.env.HOME
let sharedHomeDir: string | null = null

afterEach(async () => {
  while (cleanup.length > 0) {
    await cleanup.pop()?.()
  }

  resetDb({ filename: ":memory:" })

  if (sharedHomeDir) {
    await rm(sharedHomeDir, { recursive: true, force: true })
    sharedHomeDir = null
  }

  if (originalHome === undefined) {
    delete process.env.HOME
  } else {
    process.env.HOME = originalHome
  }
})

afterAll(async () => {
  // Per-test cleanup above already restores HOME and removes shared temp directories.
})

test("daemon submit request requires a valid session token", async () => {
  const daemon = await startServer()
  const client = createDaemonIpcClient({ daemonUrl: daemon.daemonUrl })

  const { logs } = await captureLogs(async () => {
    await expect(
      client.send("prSubmit", {
        token: "",
        cwd: process.cwd(),
        title: "Ship daemon security",
        body: "Done.",
      }),
    ).rejects.toThrow(/invalid session token/i)
  })

  const received = logs.find((entry) => entry.event === "ipc.request_received")
  const failed = logs.find((entry) => entry.event === "ipc.request_failed")
  const receivedIpcRequest = received?.ipcRequest as Record<string, unknown> | undefined
  const failedIpcRequest = failed?.ipcRequest as Record<string, unknown> | undefined
  expect(received?.requestName).toBe("prSubmit")
  expect(received?.payload).toEqual({
    token: "[REDACTED]",
    cwd: process.cwd(),
    title: "Ship daemon security",
    body: "Done.",
  })
  expect(receivedIpcRequest?.opId).toBe(failedIpcRequest?.opId)
  expect(failed?.requestName).toBe("prSubmit")
})

test("daemon hides unexpected handler crashes from IPC clients", async () => {
  await useTempHome()
  const repoDir = await createGitRepoFixture({
    owner: "trusted",
    repo: "widgets",
    branch: "feature/secure-daemon",
  })

  const daemon = await startServer({
    sdk: {
      pr: {
        create: async () => {
          throw new Error("github exploded")
        },
        reply: async () => ({ success: true }),
      },
    },
    useExistingHome: true,
  })
  seedAuthorizedSession({
    sessionId: "ses_crash",
    token: "tok_session",
    owner: "trusted",
    repo: "widgets",
    allowedPrNumbers: [],
  })

  const client = createDaemonIpcClient({ daemonUrl: daemon.daemonUrl })
  const { logs } = await captureLogs(async () => {
    await expect(
      client.send("prSubmit", {
        token: "tok_session",
        cwd: repoDir,
        title: "Ship daemon security",
        body: "Done.",
      }),
    ).rejects.toThrow(/internal server error/i)
  })

  const failed = logs.find((entry) => entry.event === "ipc.request_failed")
  expect(failed?.requestName).toBe("prSubmit")
  expect(failed?.errorMessage).toBe("github exploded")
})

test("daemon submit request enforces trusted repo context and records created PR access", async () => {
  await useTempHome()
  const repoDir = await createGitRepoFixture({
    owner: "evil",
    repo: "fork",
    branch: "feature/secure-daemon",
  })

  const createCalls: Array<Record<string, unknown>> = []

  const daemon = await startServer({
    sdk: {
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
        create: async (input) => {
          createCalls.push(input)
          return {
            number: 42,
            url: "https://github.com/trusted/widgets/pull/42",
          }
        },
        reply: async () => ({ success: true }),
      },
    },
    useExistingHome: true,
  })
  seedAuthorizedSession({
    sessionId: "ses_42",
    token: "tok_session",
    owner: "trusted",
    repo: "widgets",
    allowedPrNumbers: [],
  })

  const client = createDaemonIpcClient({ daemonUrl: daemon.daemonUrl })
  const { logs } = await captureLogs(async () => {
    await client.send("prSubmit", {
      token: "tok_session",
      cwd: repoDir,
      title: "Ship daemon security",
      body: "Done.",
    })
  })

  expect(createCalls).toEqual([
    {
      owner: "trusted",
      repo: "widgets",
      title: "Ship daemon security",
      body: "Done.",
      head: "feature/secure-daemon",
      base: "main",
    },
  ])
  expect(db.sessions.get("ses_42")?.permissions?.allowedPrNumbers).toEqual([42])
  expect(
    db.pullRequests.findMany().map(({ host, owner, repo, prNumber, cwd }) => ({
      host,
      owner,
      repo,
      prNumber,
      cwd,
    })),
  ).toEqual([
    {
      host: "github",
      owner: "trusted",
      repo: "widgets",
      prNumber: 42,
      cwd: repoDir,
    },
  ])
  const pullRequest = db.pullRequests.first({
    where: {
      host: "github",
      owner: "trusted",
      repo: "widgets",
      prNumber: 42,
    },
  })
  expect(db.inboxItems.first({ where: { entityId: pullRequest!.id } })).toMatchObject({
    entityId: pullRequest!.id,
    reason: "pull_request.created",
    status: "unread",
    priority: "normal",
    scope: "Session",
    headline: "Ship daemon security",
  })
  await expect(client.send("prGet", { id: pullRequest!.id })).resolves.toMatchObject({
    pullRequest: {
      id: pullRequest!.id,
      owner: "trusted",
      repo: "widgets",
      prNumber: 42,
    },
  })

  const received = logs.find((entry) => entry.event === "ipc.request_received")
  const responded = logs.find((entry) => entry.event === "ipc.response_sent")
  const receivedIpcRequest = received?.ipcRequest as Record<string, unknown> | undefined
  const respondedIpcRequest = responded?.ipcRequest as Record<string, unknown> | undefined
  expect(received?.requestName).toBe("prSubmit")
  expect(responded?.requestName).toBe("prSubmit")
  expect(receivedIpcRequest?.opId).toBe(respondedIpcRequest?.opId)
  expect(receivedIpcRequest?.sessionId).toBeNull()
  expect(respondedIpcRequest?.sessionId).toBe("ses_42")
})

test("daemon reply request rejects PRs outside the session allowlist", async () => {
  await useTempHome()
  const repoDir = await createGitRepoFixture({
    owner: "evil",
    repo: "fork",
    branch: "pr-12",
  })

  const daemon = await startServer({ useExistingHome: true })
  seedAuthorizedSession({
    sessionId: "ses_7",
    token: "tok_session",
    owner: "trusted",
    repo: "widgets",
    allowedPrNumbers: [7],
  })

  const client = createDaemonIpcClient({ daemonUrl: daemon.daemonUrl })
  await expect(
    client.send("prReply", {
      token: "tok_session",
      cwd: repoDir,
      message: "Updated per review",
    }),
  ).rejects.toThrow(/not allowed/i)
})

test("daemon reply request records pull request checkout locations", async () => {
  await useTempHome()
  const repoDir = await createGitRepoFixture({
    owner: "evil",
    repo: "fork",
    branch: "pr-12",
  })

  const daemon = await startServer({ useExistingHome: true })
  seedAuthorizedSession({
    sessionId: "ses_12",
    token: "tok_session",
    owner: "trusted",
    repo: "widgets",
    allowedPrNumbers: [12],
  })

  const client = createDaemonIpcClient({ daemonUrl: daemon.daemonUrl })
  await client.send("prReply", {
    token: "tok_session",
    cwd: repoDir,
    message: "Updated per review",
  })

  expect(
    db.pullRequests.findMany().map(({ host, owner, repo, prNumber, cwd }) => ({
      host,
      owner,
      repo,
      prNumber,
      cwd,
    })),
  ).toEqual([
    {
      host: "github",
      owner: "trusted",
      repo: "widgets",
      prNumber: 12,
      cwd: repoDir,
    },
  ])
  const pullRequest = db.pullRequests.first({
    where: {
      host: "github",
      owner: "trusted",
      repo: "widgets",
      prNumber: 12,
    },
  })
  expect(db.inboxItems.first({ where: { entityId: pullRequest!.id } })).toMatchObject({
    entityId: pullRequest!.id,
    reason: "pull_request.updated",
    status: "unread",
    priority: "normal",
    scope: "Session",
    headline: "PR reply posted",
  })
})

test("daemon session reporting creates and updates session inbox rows", async () => {
  await useTempHome()
  const daemon = await startServer({ useExistingHome: true })
  seedAuthorizedSession({
    sessionId: "ses_inbox",
    token: "tok_session",
    owner: "trusted",
    repo: "widgets",
    allowedPrNumbers: [],
  })

  const client = createDaemonIpcClient({ daemonUrl: daemon.daemonUrl })
  await client.send("sessionReportBlocker", {
    id: "ses_inbox",
    reason: "Need product decision",
    scope: "Checkout flow",
    headline: "Decision blocks final step",
  })

  const blockedItem = db.inboxItems.first({ where: { entityId: "ses_inbox" } })
  expect(blockedItem).toMatchObject({
    entityId: "ses_inbox",
    reason: "session.blocked",
    status: "unread",
    scope: "Checkout flow",
    headline: "Decision blocks final step",
  })

  await client.send("inboxUpdate", {
    entityId: "ses_inbox",
    status: "read",
  })
  expect(db.inboxItems.first({ where: { entityId: "ses_inbox" } })?.status).toBe("read")
  await client.send("sessionComplete", { id: "ses_inbox" })
  expect(db.inboxItems.first({ where: { entityId: "ses_inbox" } })?.status).toBe("completed")
})

test("daemon workforce request rejects mismatched roots for token-backed sessions", async () => {
  await useTempHome()
  const sessionId = db.sessions.newId()
  const token = "workforce-token-mismatch"
  const rootDir = await mkdtemp(join(tmpdir(), "goddard-workforce-root-a-"))
  const otherRootDir = await mkdtemp(join(tmpdir(), "goddard-workforce-root-b-"))
  cleanup.push(() => rm(rootDir, { recursive: true, force: true }))
  cleanup.push(() => rm(otherRootDir, { recursive: true, force: true }))

  const daemon = await startServer({ useExistingHome: true })
  await seedWorkforceSession({
    sessionId,
    token,
    rootDir,
    requestId: "req-mismatch",
  })

  const client = createDaemonIpcClient({ daemonUrl: daemon.daemonUrl })
  await expect(
    client.send("workforceRequest", {
      rootDir: otherRootDir,
      targetAgentId: "root",
      input: "Ship it.",
      token,
    }),
  ).rejects.toThrow(/does not match requested root/i)
})

test("daemon workforce respond rejects mismatched roots for token-backed sessions", async () => {
  await useTempHome()
  const sessionId = db.sessions.newId()
  const token = "workforce-token-respond"
  const rootDir = await mkdtemp(join(tmpdir(), "goddard-workforce-root-c-"))
  const otherRootDir = await mkdtemp(join(tmpdir(), "goddard-workforce-root-d-"))
  cleanup.push(() => rm(rootDir, { recursive: true, force: true }))
  cleanup.push(() => rm(otherRootDir, { recursive: true, force: true }))

  const daemon = await startServer({ useExistingHome: true })
  await seedWorkforceSession({
    sessionId,
    token,
    rootDir,
    requestId: "req-respond",
  })

  const client = createDaemonIpcClient({ daemonUrl: daemon.daemonUrl })
  await expect(
    client.send("workforceRespond", {
      rootDir: otherRootDir,
      output: "done",
      token,
    }),
  ).rejects.toThrow(/does not match requested root/i)
})

test("daemon workforce request rejects token-backed sessions without a workforce root", async () => {
  await useTempHome()
  const sessionId = db.sessions.newId()
  const token = "workforce-token-no-root"
  const rootDir = await mkdtemp(join(tmpdir(), "goddard-workforce-root-e-"))
  cleanup.push(() => rm(rootDir, { recursive: true, force: true }))

  const daemon = await startServer({ useExistingHome: true })
  await seedWorkforceSession({
    sessionId,
    token,
    rootDir,
    requestId: "req-no-root",
    includeRootDir: false,
  })

  const client = createDaemonIpcClient({ daemonUrl: daemon.daemonUrl })
  await expect(
    client.send("workforceRequest", {
      rootDir,
      targetAgentId: "root",
      input: "Ship it.",
      token,
    }),
  ).rejects.toThrow(/not attached to a workforce root/i)
})

type StartServerOptions = {
  useExistingHome?: boolean
  sdk?: {
    auth?: Partial<BackendPrClient["auth"]>
    pr?: Partial<BackendPrClient["pr"]>
  }
}

async function startServer(options: StartServerOptions = {}): Promise<DaemonServer> {
  if (!options.useExistingHome) {
    await useTempHome()
  }

  const daemon = await startDaemonServer(
    {
      auth: {
        startDeviceFlow:
          options.sdk?.auth?.startDeviceFlow ??
          (async () => ({
            deviceCode: "dev_1",
            userCode: "ABCD-1234",
            verificationUri: "https://github.com/login/device",
            expiresIn: 900,
            interval: 5,
          })),
        completeDeviceFlow:
          options.sdk?.auth?.completeDeviceFlow ??
          (async () => ({
            token: "tok_1",
            githubUsername: "alec",
            githubUserId: 42,
          })),
        whoami:
          options.sdk?.auth?.whoami ??
          (async () => ({
            token: "tok_1",
            githubUsername: "alec",
            githubUserId: 42,
          })),
        logout: options.sdk?.auth?.logout ?? (async () => {}),
      },
      pr: {
        create:
          options.sdk?.pr?.create ??
          (async () => ({
            number: 12,
            url: "https://github.com/trusted/widgets/pull/12",
          })),
        reply: options.sdk?.pr?.reply ?? (async () => ({ success: true })),
      },
    },
    { port: 0 },
  )

  cleanup.push(async () => {
    await daemon.close()
  })

  return daemon
}

async function useTempHome(): Promise<void> {
  sharedHomeDir ??= await mkdtemp(join(tmpdir(), "goddard-daemon-ipc-home-"))
  process.env.HOME = sharedHomeDir
  resetDb()
}

async function seedWorkforceSession(input: {
  sessionId: DaemonSession["id"]
  token: string
  rootDir: string
  requestId: string
  includeRootDir?: boolean
}): Promise<void> {
  const sessionRecord = {
    acpSessionId: `acp-${input.sessionId}`,
    status: "active",
    stopReason: null,
    agent: "pi-acp",
    agentName: "pi",
    cwd: input.rootDir,
    title: "New session",
    titleState: "placeholder",
    mcpServers: [],
    connectionMode: "none",
    supportsLoadSession: false,
    activeDaemonSession: false,
    errorMessage: null,
    blockedReason: null,
    initiative: null,
    inboxScope: null,
    lastAgentMessage: null,
    repository: null,
    prNumber: null,
    token: input.token,
    permissions: {
      owner: "trusted",
      repo: "widgets",
      allowedPrNumbers: [],
    },
    metadata: null,
    models: null,
    availableCommands: [],
  } satisfies Parameters<typeof db.sessions.put>[1]
  db.sessions.put(input.sessionId, sessionRecord)
  db.workforces.create({
    sessionId: input.sessionId,
    rootDir: input.includeRootDir === false ? undefined : input.rootDir,
    agentId: "root",
    requestId: input.requestId,
  })
}

function seedAuthorizedSession(input: {
  sessionId: DaemonSession["id"]
  token: string
  owner: string
  repo: string
  allowedPrNumbers: number[]
}) {
  db.sessions.put(input.sessionId, {
    acpSessionId: `acp-${input.sessionId}`,
    status: "active",
    stopReason: null,
    agent: "pi-acp",
    agentName: "pi",
    cwd: process.cwd(),
    title: "New session",
    titleState: "placeholder",
    mcpServers: [],
    connectionMode: "none",
    supportsLoadSession: false,
    activeDaemonSession: false,
    errorMessage: null,
    blockedReason: null,
    initiative: null,
    inboxScope: null,
    lastAgentMessage: null,
    repository: null,
    prNumber: null,
    token: input.token,
    permissions: {
      owner: input.owner,
      repo: input.repo,
      allowedPrNumbers: input.allowedPrNumbers,
    },
    metadata: null,
    models: null,
    availableCommands: [],
  })
}

async function createGitRepoFixture(input: {
  owner: string
  repo: string
  branch: string
}): Promise<string> {
  const repoDir = await mkdtemp(join(tmpdir(), "goddard-daemon-ipc-repo-"))
  cleanup.push(() => rm(repoDir, { recursive: true, force: true }))
  await writeFile(join(repoDir, "README.md"), "# fixture\n", "utf8")
  runGit(repoDir, ["init"])
  runGit(repoDir, ["config", "user.email", "bot@example.com"])
  runGit(repoDir, ["config", "user.name", "Bot"])
  runGit(repoDir, ["add", "README.md"])
  runGit(repoDir, ["commit", "-m", "init"])
  runGit(repoDir, ["checkout", "-b", input.branch])
  runGit(repoDir, [
    "remote",
    "add",
    "origin",
    `https://github.com/${input.owner}/${input.repo}.git`,
  ])
  return repoDir
}

function runGit(cwd: string, args: string[]) {
  const result = spawnSync("git", args, {
    cwd,
    encoding: "utf8",
  })

  expect(result.status).toBe(0)
}

async function captureLogs(
  action: () => Promise<void>,
): Promise<{ logs: Array<Record<string, unknown>> }> {
  const output: string[] = []
  const restoreLogging = configureLogging({
    mode: "json",
    writeLine: (line) => {
      output.push(line)
    },
  })

  try {
    await action()
    return {
      logs: output
        .flatMap((chunk) => chunk.split("\n"))
        .filter((line) => line.trim().length > 0)
        .map((line) => JSON.parse(line) as Record<string, unknown>),
    }
  } finally {
    restoreLogging()
  }
}
