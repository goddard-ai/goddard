import { createDaemonIpcClient } from "@goddard-ai/daemon-client/node"
import { IpcClientError } from "@goddard-ai/ipc"
import type { CreateSessionRequest, WorkforceEventEnvelope } from "@goddard-ai/schema/daemon"
import { afterEach, expect, test } from "bun:test"
import { spawnSync } from "node:child_process"
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { startDaemonServer } from "../src/ipc.ts"
import { configureLogging } from "../src/logging.ts"
import { createWorkforceManager } from "../src/workforce/manager.ts"
import { normalizeWorkforceRootDir } from "../src/workforce/paths.ts"
import { WorkforceRuntime } from "../src/workforce/runtime.ts"

const cleanup: Array<() => Promise<void>> = []

afterEach(async () => {
  while (cleanup.length > 0) {
    await cleanup.pop()?.()
  }
})

test("daemon IPC exposes repo-root workforce lifecycle methods", async () => {
  const socketDir = await mkdtemp(join(tmpdir(), "goddard-workforce-ipc-"))
  let publishEvent: ((payload: WorkforceEventEnvelope) => void) | undefined
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
        whoami: async () => ({ token: "tok_1", githubUsername: "alec", githubUserId: 42 }),
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
      createWorkforceManager: (input) => {
        publishEvent = input.publishEvent
        return {
          startWorkforce: async (rootDir: string) => ({
            state: "running",
            rootDir,
            configPath: `${rootDir}/.goddard/workforce.json`,
            ledgerPath: `${rootDir}/.goddard/ledger.jsonl`,
            activeRequestCount: 0,
            queuedRequestCount: 0,
            suspendedRequestCount: 0,
            failedRequestCount: 0,
            config: {
              version: 1,
              defaultAgent: "pi-acp",
              rootAgentId: "root",
              agents: [],
            },
          }),
          getWorkforce: async (rootDir: string) => ({
            state: "running",
            rootDir,
            configPath: `${rootDir}/.goddard/workforce.json`,
            ledgerPath: `${rootDir}/.goddard/ledger.jsonl`,
            activeRequestCount: 0,
            queuedRequestCount: 0,
            suspendedRequestCount: 0,
            failedRequestCount: 0,
            config: {
              version: 1,
              defaultAgent: "pi-acp",
              rootAgentId: "root",
              agents: [],
            },
          }),
          listWorkforces: async () => [
            {
              state: "running",
              rootDir: "/repo",
              configPath: "/repo/.goddard/workforce.json",
              ledgerPath: "/repo/.goddard/ledger.jsonl",
              activeRequestCount: 0,
              queuedRequestCount: 0,
              suspendedRequestCount: 0,
              failedRequestCount: 0,
            },
          ],
          shutdownWorkforce: async () => true,
          appendWorkforceEvent: async () => ({
            workforce: {
              state: "running",
              rootDir: "/repo",
              configPath: "/repo/.goddard/workforce.json",
              ledgerPath: "/repo/.goddard/ledger.jsonl",
              activeRequestCount: 0,
              queuedRequestCount: 1,
              suspendedRequestCount: 0,
              failedRequestCount: 0,
            },
            requestId: "req-1",
          }),
          close: async () => {},
        }
      },
    },
  )
  cleanup.push(async () => {
    await daemon.close()
    await rm(socketDir, { recursive: true, force: true })
  })

  const client = createDaemonIpcClient({ daemonUrl: daemon.daemonUrl })
  const started = await client.send("workforceStart", { rootDir: "/repo" })
  const fetched = await client.send("workforceGet", { rootDir: "/repo" })
  const listed = await client.send("workforceList")
  let resolveStreamEvent: ((payload: WorkforceEventEnvelope["event"]) => void) | null = null
  const streamedEvent = new Promise<WorkforceEventEnvelope["event"]>((resolve) => {
    resolveStreamEvent = resolve
  })
  const unsubscribe = await client.subscribe(
    { name: "workforceEvent", filter: { rootDir: "/repo" } },
    (payload) => {
      resolveStreamEvent?.(payload.event)
    },
  )
  cleanup.push(async () => {
    await Promise.resolve(unsubscribe()).catch(() => {})
  })
  const requested = await client.send("workforceRequest", {
    rootDir: "/repo",
    targetAgentId: "api",
    input: "Ship it.",
  })

  if (publishEvent) {
    publishEvent({
      rootDir: "/repo",
      event: {
        id: "evt-1",
        at: new Date().toISOString(),
        type: "request",
        requestId: "req-stream-1",
        toAgentId: "api",
        fromAgentId: null,
        intent: "default",
        input: "Tail this request.",
      },
    })
  }
  const stopped = await client.send("workforceShutdown", { rootDir: "/repo" })

  expect(started.workforce.rootDir).toBe("/repo")
  expect(fetched.workforce.rootDir).toBe("/repo")
  expect(listed.workforces).toHaveLength(1)
  await expect(streamedEvent).resolves.toMatchObject({
    id: "evt-1",
    type: "request",
    requestId: "req-stream-1",
    toAgentId: "api",
    input: "Tail this request.",
  })
  expect(requested.requestId).toBe("req-1")
  expect(stopped.success).toBe(true)
})

test("daemon workforce event stream rejects inactive repositories", async () => {
  const socketDir = await mkdtemp(join(tmpdir(), "goddard-workforce-stream-"))
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
        whoami: async () => ({ token: "tok_1", githubUsername: "alec", githubUserId: 42 }),
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
      createWorkforceManager: () => ({
        startWorkforce: async () => {
          throw new Error("not used")
        },
        getWorkforce: async (rootDir: string) => {
          throw new IpcClientError(`No workforce is running for ${rootDir}`)
        },
        listWorkforces: async () => [],
        shutdownWorkforce: async () => false,
        appendWorkforceEvent: async () => {
          throw new Error("not used")
        },
        close: async () => {},
      }),
    },
  )
  cleanup.push(async () => {
    await daemon.close()
    await rm(socketDir, { recursive: true, force: true })
  })

  const client = createDaemonIpcClient({ daemonUrl: daemon.daemonUrl })

  await expect(
    client.subscribe({ name: "workforceEvent", filter: { rootDir: "/repo" } }, () => {}),
  ).rejects.toThrow("No workforce is running for /repo")
})

test("daemon IPC discovers and initializes workforce config through daemon-owned handlers", async () => {
  const repoDir = await mkdtemp(join(tmpdir(), "goddard-workforce-init-"))
  const packageDir = join(repoDir, "packages", "ui")
  const socketDir = await mkdtemp(join(tmpdir(), "goddard-workforce-init-ipc-"))
  cleanup.push(() => rm(repoDir, { recursive: true, force: true }))

  await mkdir(packageDir, { recursive: true })
  await writeFile(
    join(repoDir, "package.json"),
    JSON.stringify({ name: "@repo/root", private: true }, null, 2),
    "utf-8",
  )
  await writeFile(
    join(packageDir, "package.json"),
    JSON.stringify({ name: "@repo/ui", private: true }, null, 2),
    "utf-8",
  )
  expect(spawnSync("git", ["init"], { cwd: repoDir }).status).toBe(0)

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
        whoami: async () => ({ token: "tok_1", githubUsername: "alec", githubUserId: 42 }),
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
  )
  cleanup.push(async () => {
    await daemon.close()
    await rm(socketDir, { recursive: true, force: true })
  })

  const client = createDaemonIpcClient({ daemonUrl: daemon.daemonUrl })
  const discovered = await client.send("workforceDiscoverCandidates", {
    rootDir: packageDir,
  })
  const normalizedRootDir = await normalizeWorkforceRootDir(repoDir)

  expect(discovered.rootDir).toBe(normalizedRootDir)
  expect(discovered.candidates.map((candidate) => candidate.relativeDir)).toEqual([
    ".",
    "packages/ui",
  ])

  const initialized = await client.send("workforceInitialize", {
    rootDir: packageDir,
    packageDirs: discovered.candidates.map((candidate) => candidate.rootDir),
  })

  const config = JSON.parse(await readFile(initialized.initialized.configPath, "utf-8")) as {
    rootAgentId: string
    agents: Array<{ id: string; cwd: string }>
  }

  expect(initialized.initialized.rootDir).toBe(normalizedRootDir)
  expect(config.rootAgentId).toBe("root")
  expect(config.agents.map((agent) => agent.cwd)).toEqual([".", "packages/ui"])
  await expect(readFile(initialized.initialized.ledgerPath, "utf-8")).resolves.toBe("")
})

test("workforce manager reuses one runtime per normalized repository root", async () => {
  const created: string[] = []
  const manager = createWorkforceManager({
    sessionManager: {} as never,
    createRuntime: async (rootDir) => {
      created.push(rootDir)
      return {
        getWorkforce: () => ({
          state: "running",
          rootDir,
          configPath: `${rootDir}/.goddard/workforce.json`,
          ledgerPath: `${rootDir}/.goddard/ledger.jsonl`,
          activeRequestCount: 0,
          queuedRequestCount: 0,
          suspendedRequestCount: 0,
          failedRequestCount: 0,
          config: {
            version: 1,
            defaultAgent: "pi-acp",
            rootAgentId: "root",
            agents: [],
          },
        }),
        getStatus: () => ({
          state: "running",
          rootDir,
          configPath: `${rootDir}/.goddard/workforce.json`,
          ledgerPath: `${rootDir}/.goddard/ledger.jsonl`,
          activeRequestCount: 0,
          queuedRequestCount: 0,
          suspendedRequestCount: 0,
          failedRequestCount: 0,
        }),
        stop: async () => {},
      } as unknown as WorkforceRuntime
    },
  })

  const tempRoot = await mkdtemp(join(tmpdir(), "goddard-workforce-manager-"))
  cleanup.push(() => rm(tempRoot, { recursive: true, force: true }))

  await manager.startWorkforce(tempRoot)
  await manager.startWorkforce(tempRoot)

  expect(created).toEqual([await normalizeWorkforceRootDir(tempRoot)])
})

test("workforce runtime records responses, suspensions, and poison-pill errors in the ledger", async () => {
  const rootDir = await mkdtemp(join(tmpdir(), "goddard-workforce-runtime-"))
  cleanup.push(() => rm(rootDir, { recursive: true, force: true }))
  await mkdir(join(rootDir, ".goddard"), { recursive: true })
  await writeFile(
    join(rootDir, ".goddard", "workforce.json"),
    JSON.stringify(
      {
        version: 1,
        defaultAgent: "pi-acp",
        rootAgentId: "root",
        agents: [
          {
            id: "root",
            name: "@repo/root",
            role: "root",
            cwd: ".",
            owns: ["."],
          },
        ],
      },
      null,
      2,
    ),
    "utf-8",
  )
  await writeFile(join(rootDir, ".goddard", "ledger.jsonl"), "", "utf-8")

  let runtime!: WorkforceRuntime
  let callCount = 0

  runtime = await WorkforceRuntime.start(rootDir, {
    sessionManager: {} as never,
    runSession: async ({ request }) => {
      callCount += 1

      if (request.input === "suspend me") {
        await runtime.suspend({
          requestId: request.id,
          reason: "Need a root decision.",
          actor: {
            sessionId: "session-1",
            rootDir: null,
            agentId: "root",
            requestId: request.id,
          },
        })
        return
      }

      if (request.input === "fail me") {
        return
      }

      await runtime.respond({
        requestId: request.id,
        output: `completed:${request.input}`,
        actor: {
          sessionId: "session-1",
          rootDir: null,
          agentId: "root",
          requestId: request.id,
        },
      })
    },
  })

  await runtime.createRequest({
    targetAgentId: "root",
    payload: "complete me",
    actor: { sessionId: null, rootDir: null, agentId: null, requestId: null },
  })
  await runtime.createRequest({
    targetAgentId: "root",
    payload: "suspend me",
    actor: { sessionId: null, rootDir: null, agentId: null, requestId: null },
  })
  await runtime.createRequest({
    targetAgentId: "root",
    payload: "fail me",
    actor: { sessionId: null, rootDir: null, agentId: null, requestId: null },
  })

  await waitFor(async () => {
    const status = runtime.getStatus()
    return status.suspendedRequestCount === 1 && status.failedRequestCount === 1
  })

  const ledger = await readFile(join(rootDir, ".goddard", "ledger.jsonl"), "utf-8")

  expect(ledger).toMatch(/"type":"response"/)
  expect(ledger).toMatch(/"type":"suspend"/)
  expect(ledger).toMatch(/"type":"error"/)
  expect(callCount).toBeGreaterThanOrEqual(5)
})

test("domain agents can update and cancel requests they originally sent", async () => {
  const rootDir = await mkdtemp(join(tmpdir(), "goddard-workforce-domain-manage-"))
  cleanup.push(() => rm(rootDir, { recursive: true, force: true }))
  await mkdir(join(rootDir, ".goddard"), { recursive: true })
  await writeFile(
    join(rootDir, ".goddard", "workforce.json"),
    JSON.stringify(
      {
        version: 1,
        defaultAgent: "pi-acp",
        rootAgentId: "root",
        agents: [
          {
            id: "root",
            name: "@repo/root",
            role: "root",
            cwd: ".",
            owns: ["."],
          },
          {
            id: "api",
            name: "@repo/api",
            role: "domain",
            cwd: "packages/api",
            owns: ["packages/api"],
          },
          {
            id: "ui",
            name: "@repo/ui",
            role: "domain",
            cwd: "packages/ui",
            owns: ["packages/ui"],
          },
        ],
      },
      null,
      2,
    ),
    "utf-8",
  )
  await writeFile(join(rootDir, ".goddard", "ledger.jsonl"), "", "utf-8")

  const runtime = await WorkforceRuntime.start(rootDir, {
    sessionManager: {} as never,
    runSession: async () => {},
  })

  const requestId = await runtime.createRequest({
    targetAgentId: "ui",
    payload: "Implement the dialog.",
    actor: {
      sessionId: "session-api",
      rootDir: null,
      agentId: "api",
      requestId: "req-api-parent",
    },
  })

  await runtime.updateRequest({
    requestId,
    payload: "Use the shared modal primitives.",
    actor: {
      sessionId: "session-api",
      rootDir: null,
      agentId: "api",
      requestId: "req-api-parent",
    },
  })

  await expect(
    runtime.cancelRequest({
      requestId,
      reason: "Wrong owner for this work.",
      actor: {
        sessionId: "session-root",
        rootDir: null,
        agentId: "ui",
        requestId: "req-ui-parent",
      },
    }),
  ).rejects.toThrow(
    "Only the root agent, the original sending agent, or an operator can cancel workforce requests",
  )

  await runtime.cancelRequest({
    requestId,
    reason: "Wrong owner for this work.",
    actor: {
      sessionId: "session-api",
      rootDir: null,
      agentId: "api",
      requestId: "req-api-parent",
    },
  })

  const ledger = await readFile(join(rootDir, ".goddard", "ledger.jsonl"), "utf-8")

  expect(ledger).toMatch(new RegExp(`"requestId":"${requestId}"`))
  expect(ledger).toMatch(/"type":"update"/)
  expect(ledger).toMatch(/"type":"cancel"/)
})

test("buildSystemPrompt warns agents about off-limits paths owned by other agents", async () => {
  const rootDir = await mkdtemp(join(tmpdir(), "goddard-workforce-limits-"))
  cleanup.push(() => rm(rootDir, { recursive: true, force: true }))
  await mkdir(join(rootDir, ".goddard"), { recursive: true })
  await writeFile(
    join(rootDir, ".goddard", "workforce.json"),
    JSON.stringify(
      {
        version: 1,
        defaultAgent: "pi-acp",
        rootAgentId: "root",
        agents: [
          {
            id: "root",
            name: "@repo/root",
            role: "root",
            cwd: ".",
            owns: ["."],
          },
          {
            id: "lib",
            name: "@repo/lib",
            role: "domain",
            cwd: "packages/lib",
            owns: ["packages/lib"],
          },
          {
            id: "foo",
            name: "@repo/foo",
            role: "domain",
            cwd: "packages/foo",
            owns: ["packages/foo"],
          },
        ],
      },
      null,
      2,
    ),
    "utf-8",
  )
  await writeFile(join(rootDir, ".goddard", "ledger.jsonl"), "", "utf-8")

  let runtime!: WorkforceRuntime
  const systemPrompts: Record<string, string> = {}

  runtime = await WorkforceRuntime.start(rootDir, {
    sessionManager: {
      newSession: async (input: { request: CreateSessionRequest }) => {
        const metadata = input.request.workforce ?? null

        if (!metadata?.agentId || !metadata.requestId) {
          throw new Error("Missing workforce metadata")
        }

        systemPrompts[metadata.agentId] = input.request.systemPrompt

        await runtime.respond({
          requestId: metadata.requestId,
          output: "ok",
          actor: {
            sessionId: "session-1",
            rootDir: null,
            agentId: metadata.agentId,
            requestId: metadata.requestId,
          },
        })

        return {} as never
      },
    } as never,
  })

  await runtime.createRequest({
    targetAgentId: "root",
    payload: "Do root work.",
    actor: { sessionId: null, rootDir: null, agentId: null, requestId: null },
  })

  await runtime.createRequest({
    targetAgentId: "lib",
    payload: "Do lib work.",
    actor: { sessionId: null, rootDir: null, agentId: null, requestId: null },
  })

  await waitFor(() => runtime.getStatus().queuedRequestCount === 0)

  expect(systemPrompts["root"]).toContain("packages/foo")
  expect(systemPrompts["root"]).toContain("packages/lib")
  expect(systemPrompts["lib"]).not.toContain("packages/foo")
})

test("create-intent requests target the root agent and specialize the root session prompt", async () => {
  const rootDir = await mkdtemp(join(tmpdir(), "goddard-workforce-create-"))
  cleanup.push(() => rm(rootDir, { recursive: true, force: true }))
  await mkdir(join(rootDir, ".goddard"), { recursive: true })
  await writeFile(
    join(rootDir, ".goddard", "workforce.json"),
    JSON.stringify(
      {
        version: 1,
        defaultAgent: "pi-acp",
        rootAgentId: "root",
        agents: [
          {
            id: "root",
            name: "@repo/root",
            role: "root",
            cwd: ".",
            owns: ["."],
          },
          {
            id: "lib",
            name: "@repo/lib",
            role: "domain",
            cwd: "packages/lib",
            owns: ["packages/lib"],
          },
        ],
      },
      null,
      2,
    ),
    "utf-8",
  )
  await writeFile(join(rootDir, ".goddard", "ledger.jsonl"), "", "utf-8")

  let runtime!: WorkforceRuntime
  let defaultSystemPrompt = ""
  let createSystemPrompt = ""
  let createInitialPrompt = ""
  let capturedEnv: Record<string, string> | undefined

  runtime = await WorkforceRuntime.start(rootDir, {
    sessionManager: {
      newSession: async (input: { request: CreateSessionRequest }) => {
        const initialPrompt =
          typeof input.request.initialPrompt === "string"
            ? input.request.initialPrompt
            : JSON.stringify(input.request.initialPrompt)
        capturedEnv = input.request.env

        if (initialPrompt.includes("Request intent: create")) {
          createSystemPrompt = input.request.systemPrompt
          createInitialPrompt = initialPrompt
        } else {
          defaultSystemPrompt = input.request.systemPrompt
        }

        const metadata = input.request.workforce ?? null

        if (!metadata?.agentId || !metadata.requestId) {
          throw new Error("Missing workforce metadata")
        }

        await runtime.respond({
          requestId: metadata.requestId,
          output: "created",
          actor: {
            sessionId: "session-1",
            rootDir: null,
            agentId: metadata.agentId,
            requestId: metadata.requestId,
          },
        })

        return {} as never
      },
    } as never,
  })

  await runtime.createRequest({
    targetAgentId: "root",
    payload: "Review the existing workspace boundaries.",
    actor: { sessionId: null, rootDir: null, agentId: null, requestId: null },
  })

  await expect(
    runtime.createRequest({
      targetAgentId: "lib",
      payload: "Create a new package for scheduling jobs.",
      intent: "create",
      actor: { sessionId: null, rootDir: null, agentId: null, requestId: null },
    }),
  ).rejects.toThrow("Create requests must target the root workforce agent")

  await runtime.createRequest({
    targetAgentId: "root",
    payload: "Create a new package for scheduling jobs.",
    intent: "create",
    actor: { sessionId: null, rootDir: null, agentId: null, requestId: null },
  })

  await waitFor(() => runtime.getStatus().queuedRequestCount === 0)

  const ledger = await readFile(join(rootDir, ".goddard", "ledger.jsonl"), "utf-8")

  expect(ledger).toMatch(/"intent":"create"/)
  expect(listAdvertisedWorkforceCommands(createSystemPrompt)).toEqual(
    listAdvertisedWorkforceCommands(defaultSystemPrompt),
  )
  expect(listAdvertisedWorkforceCommands(createSystemPrompt)).toEqual([
    "workforce cancel --request-id <request-id> [--reason-file <path>]",
    "workforce request --target-agent-id <agent-id> --input-file <path>",
    "workforce respond --output-file <path>",
    "workforce suspend --reason-file <path>",
    "workforce truncate [--agent-id <agent-id>] [--reason-file <path>]",
    "workforce update --request-id <request-id> --input-file <path>",
  ])
  expect(createSystemPrompt).not.toBe(defaultSystemPrompt)
  expect(createInitialPrompt).toContain("Request intent: create")
  expect(createInitialPrompt).not.toContain("Current request id:")
  expect(capturedEnv).not.toHaveProperty("GODDARD_WORKFORCE_REQUEST_ID")
})

test("domain-agent sessions advertise sender-owned update and cancel commands", async () => {
  const rootDir = await mkdtemp(join(tmpdir(), "goddard-workforce-domain-prompt-"))
  cleanup.push(() => rm(rootDir, { recursive: true, force: true }))
  await mkdir(join(rootDir, ".goddard"), { recursive: true })
  await writeFile(
    join(rootDir, ".goddard", "workforce.json"),
    JSON.stringify(
      {
        version: 1,
        defaultAgent: "pi-acp",
        rootAgentId: "root",
        agents: [
          {
            id: "root",
            name: "@repo/root",
            role: "root",
            cwd: ".",
            owns: ["."],
          },
          {
            id: "api",
            name: "@repo/api",
            role: "domain",
            cwd: "packages/api",
            owns: ["packages/api"],
          },
        ],
      },
      null,
      2,
    ),
    "utf-8",
  )
  await writeFile(join(rootDir, ".goddard", "ledger.jsonl"), "", "utf-8")

  let runtime!: WorkforceRuntime
  let capturedSystemPrompt = ""

  runtime = await WorkforceRuntime.start(rootDir, {
    sessionManager: {
      newSession: async (input: { request: CreateSessionRequest }) => {
        capturedSystemPrompt = input.request.systemPrompt

        const metadata = input.request.workforce ?? null

        if (!metadata?.agentId || !metadata.requestId) {
          throw new Error("Missing workforce metadata")
        }

        await runtime.respond({
          requestId: metadata.requestId,
          output: "done",
          actor: {
            sessionId: "session-1",
            rootDir: null,
            agentId: metadata.agentId,
            requestId: metadata.requestId,
          },
        })

        return {} as never
      },
    } as never,
  })

  await runtime.createRequest({
    targetAgentId: "api",
    payload: "Implement the endpoint.",
    actor: { sessionId: null, rootDir: null, agentId: null, requestId: null },
  })

  await waitFor(() => runtime.getStatus().queuedRequestCount === 0)

  expect(listAdvertisedWorkforceCommands(capturedSystemPrompt)).toEqual([
    "workforce cancel --request-id <request-id> [--reason-file <path>]",
    "workforce request --target-agent-id <agent-id> --input-file <path>",
    "workforce respond --output-file <path>",
    "workforce suspend --reason-file <path>",
    "workforce update --request-id <request-id> --input-file <path>",
  ])
})

test("workforce runtime logs request-to-session correlation for launched sessions", async () => {
  const rootDir = await mkdtemp(join(tmpdir(), "goddard-workforce-logs-"))
  cleanup.push(() => rm(rootDir, { recursive: true, force: true }))
  await mkdir(join(rootDir, ".goddard"), { recursive: true })
  await writeFile(
    join(rootDir, ".goddard", "workforce.json"),
    JSON.stringify(
      {
        version: 1,
        defaultAgent: "pi-acp",
        rootAgentId: "root",
        agents: [
          {
            id: "root",
            name: "@repo/root",
            role: "root",
            cwd: ".",
            owns: ["."],
          },
        ],
      },
      null,
      2,
    ),
    "utf-8",
  )
  await writeFile(join(rootDir, ".goddard", "ledger.jsonl"), "", "utf-8")

  let runtime!: WorkforceRuntime
  const { logs } = await captureLogs(async () => {
    runtime = await WorkforceRuntime.start(rootDir, {
      sessionManager: {
        newSession: async (input: { request: CreateSessionRequest }) => {
          const metadata = input.request.workforce ?? null

          if (!metadata?.agentId || !metadata.requestId) {
            throw new Error("Missing workforce metadata")
          }

          await runtime.respond({
            requestId: metadata.requestId,
            output: "done",
            actor: {
              sessionId: "daemon-session-1",
              rootDir: null,
              agentId: metadata.agentId,
              requestId: metadata.requestId,
            },
          })

          return {
            id: "daemon-session-1",
            acpSessionId: "acp-session-1",
            status: "done",
          } as never
        },
      } as never,
    })

    await runtime.createRequest({
      targetAgentId: "root",
      payload: "Ship the logging changes.",
      actor: { sessionId: null, rootDir: null, agentId: null, requestId: null },
    })

    await waitFor(() => runtime.getStatus().queuedRequestCount === 0)
  })

  const launchLog = logs.find((entry) => entry.event === "workforce.session_launch_started")
  expect(launchLog).toBeTruthy()
  expect((launchLog?.workforceDispatch as Record<string, unknown> | undefined)?.rootDir).toBe(
    rootDir,
  )
  expect((launchLog?.workforceDispatch as Record<string, unknown> | undefined)?.agentId).toBe(
    "root",
  )
  expect((launchLog?.workforceDispatch as Record<string, unknown> | undefined)?.attempt).toBe(1)
  expect(
    typeof (launchLog?.workforceDispatch as Record<string, unknown> | undefined)?.requestId,
  ).toBe("string")

  const completedLog = logs.find(
    (entry) =>
      entry.event === "workforce.session_completed" && entry.sessionId === "daemon-session-1",
  )
  expect(completedLog).toBeTruthy()
  expect(completedLog?.acpSessionId).toBe("acp-session-1")
  expect((completedLog?.workforceDispatch as Record<string, unknown> | undefined)?.rootDir).toBe(
    rootDir,
  )
  expect((completedLog?.workforceDispatch as Record<string, unknown> | undefined)?.agentId).toBe(
    "root",
  )
  expect(
    typeof (completedLog?.workforceDispatch as Record<string, unknown> | undefined)?.requestId,
  ).toBe("string")

  const respondedLog = logs.find((entry) => entry.event === "workforce.request_responded")
  expect(respondedLog).toBeTruthy()
  expect((respondedLog?.workforceActor as Record<string, unknown> | undefined)?.sessionId).toBe(
    "daemon-session-1",
  )
  expect((respondedLog?.workforceActor as Record<string, unknown> | undefined)?.agentId).toBe(
    "root",
  )
  expect((respondedLog?.workforceActor as Record<string, unknown> | undefined)?.requestId).toBe(
    (completedLog?.workforceDispatch as Record<string, unknown> | undefined)?.requestId,
  )
})

test("workforce runtime rejects responses and suspends for a different attached request", async () => {
  const rootDir = await mkdtemp(join(tmpdir(), "goddard-workforce-session-request-"))
  cleanup.push(() => rm(rootDir, { recursive: true, force: true }))
  await mkdir(join(rootDir, ".goddard"), { recursive: true })
  await writeFile(
    join(rootDir, ".goddard", "workforce.json"),
    JSON.stringify(
      {
        version: 1,
        defaultAgent: "pi-acp",
        rootAgentId: "root",
        agents: [
          {
            id: "root",
            name: "@repo/root",
            role: "root",
            cwd: ".",
            owns: ["."],
          },
        ],
      },
      null,
      2,
    ),
    "utf-8",
  )
  await writeFile(join(rootDir, ".goddard", "ledger.jsonl"), "", "utf-8")

  let releaseSession = () => {}
  const sessionBlocked = new Promise<void>((resolve) => {
    releaseSession = resolve
  })

  const runtime = await WorkforceRuntime.start(rootDir, {
    sessionManager: {} as never,
    runSession: async () => {
      await sessionBlocked
    },
  })

  const requestId = await runtime.createRequest({
    targetAgentId: "root",
    payload: "complete me",
    actor: { sessionId: null, rootDir: null, agentId: null, requestId: null },
  })

  await waitFor(() => runtime.getStatus().activeRequestCount === 1)

  await expect(
    runtime.respond({
      requestId,
      output: "completed",
      actor: {
        sessionId: "session-1",
        rootDir: null,
        agentId: "root",
        requestId: "req-other",
      },
    }),
  ).rejects.toThrow("Session request req-other cannot respond to")

  await expect(
    runtime.suspend({
      requestId,
      reason: "Need help.",
      actor: {
        sessionId: "session-1",
        rootDir: null,
        agentId: "root",
        requestId: "req-other",
      },
    }),
  ).rejects.toThrow("Session request req-other cannot suspend")

  releaseSession()
  await waitFor(() => runtime.getStatus().failedRequestCount === 1)
})

async function waitFor(predicate: () => boolean | Promise<boolean>, timeoutMs: number = 5_000) {
  const startedAt = Date.now()

  while (Date.now() - startedAt < timeoutMs) {
    if (await predicate()) {
      return
    }

    await new Promise((resolve) => setTimeout(resolve, 25))
  }

  throw new Error("Timed out waiting for workforce condition")
}

function listAdvertisedWorkforceCommands(prompt: string): string[] {
  return Array.from(
    new Set(
      prompt
        .split("\n")
        .map((line) => line.trim())
        .flatMap((line) => {
          const match = line.match(/^`(workforce [^`]+)`$/)
          return match ? [match[1]] : []
        }),
    ),
  ).sort()
}

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
