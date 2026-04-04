import { createDaemonIpcClient } from "@goddard-ai/daemon-client/node"
import type { DaemonSession } from "@goddard-ai/schema/daemon"
import { afterAll, afterEach, expect, test } from "bun:test"
import type { KindInput, KindOutput } from "kindstore"
import { mkdtemp, rm } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join } from "node:path"
import type { DaemonServer } from "../src/ipc.ts"
import { startDaemonServer } from "../src/ipc.ts"
import type { BackendPrClient } from "../src/ipc/types.ts"
import { configureDaemonLogging } from "../src/logging.ts"
import { db, resetDb } from "../src/persistence/store.ts"
import type { WorkforceManager, WorkforceManagerMutation } from "../src/workforce/manager.ts"
import { normalizeWorkforceRootDir } from "../src/workforce/paths.ts"
import type { WorkforceActorContext } from "../src/workforce/runtime.ts"

const cleanup: Array<() => Promise<void>> = []
const originalHome = process.env.HOME
let sharedHomeDir: string | null = null

afterEach(async () => {
  while (cleanup.length > 0) {
    await cleanup.pop()?.()
  }

  if (sharedHomeDir) {
    await rm(sharedHomeDir, { recursive: true, force: true })
    sharedHomeDir = null
  }

  if (originalHome === undefined) {
    delete process.env.HOME
  } else {
    process.env.HOME = originalHome
  }
  resetDb()
})

afterAll(async () => {
  // Per-test cleanup above already restores HOME and removes shared temp directories.
})

test("daemon submit request requires a valid session token", async () => {
  const daemon = await startTestDaemon()
  const client = createDaemonIpcClient({ daemonUrl: daemon.daemonUrl })

  const { logs } = await captureDaemonLogs(async () => {
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
  expect(received?.requestName).toBe("prSubmit")
  expect(received?.payload).toEqual({
    token: "[REDACTED]",
    cwd: process.cwd(),
    title: "Ship daemon security",
    body: "Done.",
  })
  expect(received?.opId).toBe(failed?.opId)
  expect(failed?.requestName).toBe("prSubmit")
})

test("daemon hides unexpected handler crashes from IPC clients", async () => {
  const daemon = await startTestDaemon({
    sdk: {
      pr: {
        create: async () => {
          throw new Error("github exploded")
        },
        reply: async () => ({ success: true }),
      },
    },
    auth: {
      getSessionByToken: async () => ({
        sessionId: "ses_crash",
        owner: "trusted",
        repo: "widgets",
        allowedPrNumbers: [],
      }),
      addAllowedPr: async () => undefined,
    },
    resolveSubmitRequest: async () => ({
      owner: "trusted",
      repo: "widgets",
      title: "Ship daemon security",
      body: "Done.",
      head: "feature/secure-daemon",
      base: "main",
    }),
  })

  const client = createDaemonIpcClient({ daemonUrl: daemon.daemonUrl })
  const { logs } = await captureDaemonLogs(async () => {
    await expect(
      client.send("prSubmit", {
        token: "tok_session",
        cwd: process.cwd(),
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
  const createCalls: Array<Record<string, unknown>> = []
  const recordedPrs: Array<{ sessionId: string; prNumber: number }> = []
  const recordedLocations: Array<{
    host: "github"
    owner: string
    repo: string
    prNumber: number
    cwd: string
  }> = []

  const daemon = await startTestDaemon({
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
        whoami: async () => ({ token: "tok_1", githubUsername: "alec", githubUserId: 42 }),
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
    auth: {
      getSessionByToken: async (token) => {
        expect(token).toBe("tok_session")
        return {
          sessionId: "ses_42",
          owner: "trusted",
          repo: "widgets",
          allowedPrNumbers: [],
        }
      },
      addAllowedPr: async (sessionId, prNumber) => {
        recordedPrs.push({ sessionId, prNumber })
      },
    },
    recordPullRequest: async (record) => {
      recordedLocations.push(record)
      return {
        id: db.pullRequests.newId(),
        ...record,
        updatedAt: Date.now(),
      }
    },
    resolveSubmitRequest: async () => ({
      owner: "evil",
      repo: "fork",
      title: "Ship daemon security",
      body: "Done.",
      head: "feature/secure-daemon",
      base: "main",
    }),
  })

  const client = createDaemonIpcClient({ daemonUrl: daemon.daemonUrl })
  const { logs } = await captureDaemonLogs(async () => {
    await client.send("prSubmit", {
      token: "tok_session",
      cwd: process.cwd(),
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
  expect(recordedPrs).toEqual([{ sessionId: "ses_42", prNumber: 42 }])
  expect(recordedLocations).toEqual([
    {
      host: "github",
      owner: "trusted",
      repo: "widgets",
      prNumber: 42,
      cwd: process.cwd(),
    },
  ])

  const received = logs.find((entry) => entry.event === "ipc.request_received")
  const responded = logs.find((entry) => entry.event === "ipc.response_sent")
  expect(received?.requestName).toBe("prSubmit")
  expect(responded?.requestName).toBe("prSubmit")
  expect(received?.opId).toBe(responded?.opId)
  expect(responded?.sessionId).toBe("ses_42")
})

test("daemon reply request rejects PRs outside the session allowlist", async () => {
  const daemon = await startTestDaemon({
    auth: {
      getSessionByToken: async () => ({
        sessionId: "ses_7",
        owner: "trusted",
        repo: "widgets",
        allowedPrNumbers: [7],
      }),
      addAllowedPr: async () => undefined,
    },
    resolveReplyRequest: async () => ({
      owner: "trusted",
      repo: "widgets",
      prNumber: 12,
      body: "Updated per review",
    }),
  })

  const client = createDaemonIpcClient({ daemonUrl: daemon.daemonUrl })
  await expect(
    client.send("prReply", {
      token: "tok_session",
      cwd: process.cwd(),
      message: "Updated per review",
    }),
  ).rejects.toThrow(/not allowed/i)
})

test("daemon reply request records pull request checkout locations", async () => {
  const recordedLocations: Array<{
    host: "github"
    owner: string
    repo: string
    prNumber: number
    cwd: string
  }> = []

  const daemon = await startTestDaemon({
    auth: {
      getSessionByToken: async () => ({
        sessionId: "ses_12",
        owner: "trusted",
        repo: "widgets",
        allowedPrNumbers: [12],
      }),
      addAllowedPr: async () => undefined,
    },
    recordPullRequest: async (record) => {
      recordedLocations.push(record)
      return {
        id: db.pullRequests.newId(),
        ...record,
        updatedAt: Date.now(),
      }
    },
    resolveReplyRequest: async () => ({
      owner: "evil",
      repo: "fork",
      prNumber: 12,
      body: "Updated per review",
    }),
  })

  const client = createDaemonIpcClient({ daemonUrl: daemon.daemonUrl })
  await client.send("prReply", {
    token: "tok_session",
    cwd: process.cwd(),
    message: "Updated per review",
  })

  expect(recordedLocations).toEqual([
    {
      host: "github",
      owner: "trusted",
      repo: "widgets",
      prNumber: 12,
      cwd: process.cwd(),
    },
  ])
})

test("daemon workforce request binds token-backed mutations to the session workforce root", async () => {
  await useTempHome()
  const sessionId = db.sessions.newId()
  const token = "workforce-token-match"
  const rootDir = await mkdtemp(join(tmpdir(), "goddard-workforce-root-match-"))
  cleanup.push(() => rm(rootDir, { recursive: true, force: true }))
  await seedWorkforceSession({
    sessionId,
    token,
    rootDir,
    requestId: "req-match",
  })

  const calls: Array<{ rootDir: string; mutationType: string; actorRootDir: string | null }> = []
  const daemon = await startTestDaemon({
    auth: {
      getSessionByToken: async (candidateToken) =>
        candidateToken === token
          ? {
              sessionId,
              owner: "trusted",
              repo: "widgets",
              allowedPrNumbers: [],
            }
          : null,
      addAllowedPr: async () => undefined,
    },
    createWorkforceManager: () =>
      createStaticWorkforceManager((rootDir, mutation, actor) => {
        calls.push({
          rootDir,
          mutationType: mutation.type,
          actorRootDir: actor.rootDir,
        })
      }),
  })

  const client = createDaemonIpcClient({ daemonUrl: daemon.daemonUrl })
  const response = await client.send("workforceRequest", {
    rootDir,
    targetAgentId: "root",
    input: "Ship it.",
    token,
  })

  const normalizedRootDir = await normalizeWorkforceRootDir(rootDir)
  expect(response.requestId).toBe("req-1")
  expect(calls).toEqual([
    {
      rootDir: normalizedRootDir,
      mutationType: "request",
      actorRootDir: normalizedRootDir,
    },
  ])
})

test("daemon workforce request rejects mismatched roots for token-backed sessions", async () => {
  await useTempHome()
  const sessionId = db.sessions.newId()
  const token = "workforce-token-mismatch"
  const rootDir = await mkdtemp(join(tmpdir(), "goddard-workforce-root-a-"))
  const otherRootDir = await mkdtemp(join(tmpdir(), "goddard-workforce-root-b-"))
  cleanup.push(() => rm(rootDir, { recursive: true, force: true }))
  cleanup.push(() => rm(otherRootDir, { recursive: true, force: true }))
  await seedWorkforceSession({
    sessionId,
    token,
    rootDir,
    requestId: "req-mismatch",
  })

  const calls: Array<{ rootDir: string }> = []
  const daemon = await startTestDaemon({
    auth: {
      getSessionByToken: async (candidateToken) =>
        candidateToken === token
          ? {
              sessionId,
              owner: "trusted",
              repo: "widgets",
              allowedPrNumbers: [],
            }
          : null,
      addAllowedPr: async () => undefined,
    },
    createWorkforceManager: () =>
      createStaticWorkforceManager((rootDir) => {
        calls.push({ rootDir })
      }),
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

  expect(calls).toEqual([])
})

test("daemon workforce respond rejects mismatched roots for token-backed sessions", async () => {
  await useTempHome()
  const sessionId = db.sessions.newId()
  const token = "workforce-token-respond"
  const rootDir = await mkdtemp(join(tmpdir(), "goddard-workforce-root-c-"))
  const otherRootDir = await mkdtemp(join(tmpdir(), "goddard-workforce-root-d-"))
  cleanup.push(() => rm(rootDir, { recursive: true, force: true }))
  cleanup.push(() => rm(otherRootDir, { recursive: true, force: true }))
  await seedWorkforceSession({
    sessionId,
    token,
    rootDir,
    requestId: "req-respond",
  })

  const daemon = await startTestDaemon({
    auth: {
      getSessionByToken: async (candidateToken) =>
        candidateToken === token
          ? {
              sessionId,
              owner: "trusted",
              repo: "widgets",
              allowedPrNumbers: [],
            }
          : null,
      addAllowedPr: async () => undefined,
    },
    createWorkforceManager: () => createStaticWorkforceManager(() => {}),
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
  await seedWorkforceSession({
    sessionId,
    token,
    rootDir,
    requestId: "req-no-root",
    includeRootDir: false,
  })

  const daemon = await startTestDaemon({
    auth: {
      getSessionByToken: async (candidateToken) =>
        candidateToken === token
          ? {
              sessionId,
              owner: "trusted",
              repo: "widgets",
              allowedPrNumbers: [],
            }
          : null,
      addAllowedPr: async () => undefined,
    },
    createWorkforceManager: () => createStaticWorkforceManager(() => {}),
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

test("daemon workforce request preserves operator mutations without a token", async () => {
  const rootDir = await mkdtemp(join(tmpdir(), "goddard-workforce-root-operator-"))
  cleanup.push(() => rm(rootDir, { recursive: true, force: true }))
  const calls: Array<{ rootDir: string; actorRootDir: string | null }> = []
  const daemon = await startTestDaemon({
    createWorkforceManager: () =>
      createStaticWorkforceManager((rootDir, _mutation, actor) => {
        calls.push({ rootDir, actorRootDir: actor.rootDir })
      }),
  })

  const client = createDaemonIpcClient({ daemonUrl: daemon.daemonUrl })
  await client.send("workforceRequest", {
    rootDir,
    targetAgentId: "root",
    input: "Ship it.",
  })

  expect(calls).toEqual([{ rootDir, actorRootDir: null }])
})

type StartTestDaemonOptions = {
  useExistingHome?: boolean
  createWorkforceManager?: NonNullable<
    Parameters<typeof startDaemonServer>[2]
  >["createWorkforceManager"]
  sdk?: {
    auth?: Partial<BackendPrClient["auth"]>
    pr?: Partial<BackendPrClient["pr"]>
  }
  auth?: {
    getSessionByToken?: (token: string) => Promise<{
      sessionId: DaemonSession["id"]
      owner: string
      repo: string
      allowedPrNumbers: number[]
    } | null>
    addAllowedPr?: (sessionId: DaemonSession["id"], prNumber: number) => Promise<void>
  }
  recordPullRequest?: (
    record: KindInput<typeof db.schema.pullRequests>,
  ) => Promise<KindOutput<typeof db.schema.pullRequests>>
  resolveSubmitRequest?: (input: any) => Promise<any>
  resolveReplyRequest?: (input: any) => Promise<any>
}

async function startTestDaemon(options: StartTestDaemonOptions = {}): Promise<DaemonServer> {
  if (!options.useExistingHome) {
    await useTempHome()
  }

  const socketDir = await mkdtemp(join(tmpdir(), "goddard-daemon-ipc-"))
  const socketPath = join(socketDir, "daemon.sock")

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
          (async () => ({ token: "tok_1", githubUsername: "alec", githubUserId: 42 })),
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
    { socketPath },
    {
      resolveSubmitRequest:
        options.resolveSubmitRequest ??
        (async () => ({
          owner: "trusted",
          repo: "widgets",
          title: "default",
          body: "",
          head: "feature/default",
          base: "main",
        })),
      resolveReplyRequest:
        options.resolveReplyRequest ??
        (async () => ({
          owner: "trusted",
          repo: "widgets",
          prNumber: 12,
          body: "reply",
        })),
      getSessionByToken: options.auth?.getSessionByToken,
      addAllowedPrToSession: options.auth?.addAllowedPr,
      recordPullRequest: options.recordPullRequest,
      createWorkforceManager: options.createWorkforceManager,
    },
  )

  cleanup.push(async () => {
    await daemon.close()
    await rm(socketDir, { recursive: true, force: true })
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
    agentName: "pi",
    cwd: input.rootDir,
    mcpServers: [],
    token: input.token,
    permissions: {
      owner: "trusted",
      repo: "widgets",
      allowedPrNumbers: [],
    },
    metadata: null,
  } satisfies Parameters<typeof db.sessions.put>[1]
  db.sessions.put(input.sessionId, sessionRecord)
  db.workforces.create({
    sessionId: input.sessionId,
    ...(input.includeRootDir === false ? {} : { rootDir: input.rootDir }),
    agentId: "root",
    requestId: input.requestId,
  })
}

function createStaticWorkforceManager(
  onAppend: (
    rootDir: string,
    mutation: WorkforceManagerMutation,
    actor: WorkforceActorContext,
  ) => void,
): WorkforceManager {
  return {
    startWorkforce: async (rootDir: string) => buildWorkforce(rootDir),
    getWorkforce: async (rootDir: string) => buildWorkforce(rootDir),
    listWorkforces: async () => [],
    shutdownWorkforce: async () => true,
    appendWorkforceEvent: async (
      rootDir: string,
      mutation: WorkforceManagerMutation,
      actor?: WorkforceActorContext,
    ) => {
      onAppend(
        rootDir,
        mutation,
        actor ?? {
          sessionId: null,
          rootDir: null,
          agentId: null,
          requestId: null,
        },
      )
      return {
        workforce: buildWorkforceStatus(rootDir),
        requestId:
          mutation.type === "request"
            ? "req-1"
            : "requestId" in mutation
              ? (mutation.requestId ?? null)
              : null,
      }
    },
    close: async () => {},
  }
}

function buildWorkforce(rootDir: string) {
  return {
    ...buildWorkforceStatus(rootDir),
    config: {
      version: 1 as const,
      defaultAgent: "pi",
      rootAgentId: "root",
      agents: [],
    },
  }
}

function buildWorkforceStatus(rootDir: string) {
  return {
    state: "running" as const,
    rootDir,
    configPath: `${rootDir}/.goddard/workforce.json`,
    ledgerPath: `${rootDir}/.goddard/ledger.jsonl`,
    activeRequestCount: 0,
    queuedRequestCount: 0,
    suspendedRequestCount: 0,
    failedRequestCount: 0,
  }
}

async function captureDaemonLogs(
  action: () => Promise<void>,
): Promise<{ logs: Array<Record<string, unknown>> }> {
  const output: string[] = []
  const restoreLogging = configureDaemonLogging({
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
