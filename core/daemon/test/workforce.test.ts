import { createDaemonIpcClient } from "@goddard-ai/daemon-client"
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { afterEach, expect, test } from "vitest"
import { startDaemonServer } from "../src/ipc.ts"
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
  const daemon = await startDaemonServer(
    {
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
            defaultAgent: "pi",
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
            defaultAgent: "pi",
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
      }),
    },
  )
  cleanup.push(async () => {
    await daemon.close()
    await rm(socketDir, { recursive: true, force: true })
  })

  const client = createDaemonIpcClient({ daemonUrl: daemon.daemonUrl })
  const started = await client.send("workforceStart", { rootDir: "/repo" })
  const fetched = await client.send("workforceGet", { rootDir: "/repo" })
  const listed = await client.send("workforceList", {})
  const requested = await client.send("workforceRequest", {
    rootDir: "/repo",
    targetAgentId: "api",
    input: "Ship it.",
  })
  const stopped = await client.send("workforceShutdown", { rootDir: "/repo" })

  expect(started.workforce.rootDir).toBe("/repo")
  expect(fetched.workforce.rootDir).toBe("/repo")
  expect(listed.workforces).toHaveLength(1)
  expect(requested.requestId).toBe("req-1")
  expect(stopped.success).toBe(true)
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
            defaultAgent: "pi",
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
        defaultAgent: "pi",
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
          agentId: "root",
          requestId: request.id,
        },
      })
    },
  })

  await runtime.createRequest({
    targetAgentId: "root",
    payload: "complete me",
    actor: { sessionId: null, agentId: null, requestId: null },
  })
  await runtime.createRequest({
    targetAgentId: "root",
    payload: "suspend me",
    actor: { sessionId: null, agentId: null, requestId: null },
  })
  await runtime.createRequest({
    targetAgentId: "root",
    payload: "fail me",
    actor: { sessionId: null, agentId: null, requestId: null },
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

test("create-intent requests target the root agent and specialize the root session prompt", async () => {
  const rootDir = await mkdtemp(join(tmpdir(), "goddard-workforce-create-"))
  cleanup.push(() => rm(rootDir, { recursive: true, force: true }))
  await mkdir(join(rootDir, ".goddard"), { recursive: true })
  await writeFile(
    join(rootDir, ".goddard", "workforce.json"),
    JSON.stringify(
      {
        version: 1,
        defaultAgent: "pi",
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
  let capturedSystemPrompt = ""
  let capturedInitialPrompt = ""

  runtime = await WorkforceRuntime.start(rootDir, {
    sessionManager: {
      createSession: async (input) => {
        capturedSystemPrompt = input.systemPrompt
        capturedInitialPrompt =
          typeof input.initialPrompt === "string"
            ? input.initialPrompt
            : JSON.stringify(input.initialPrompt)

        const metadata =
          input.metadata && typeof input.metadata === "object" && "workforce" in input.metadata
            ? (input.metadata.workforce as { requestId: string; agentId: string })
            : null

        if (!metadata) {
          throw new Error("Missing workforce metadata")
        }

        await runtime.respond({
          requestId: metadata.requestId,
          output: "created",
          actor: {
            sessionId: "session-1",
            agentId: metadata.agentId,
            requestId: metadata.requestId,
          },
        })

        return {} as never
      },
    } as never,
  })

  await expect(
    runtime.createRequest({
      targetAgentId: "lib",
      payload: "Create a new package for scheduling jobs.",
      intent: "create",
      actor: { sessionId: null, agentId: null, requestId: null },
    }),
  ).rejects.toThrow("Create requests must target the root workforce agent")

  await runtime.createRequest({
    targetAgentId: "root",
    payload: "Create a new package for scheduling jobs.",
    intent: "create",
    actor: { sessionId: null, agentId: null, requestId: null },
  })

  await waitFor(() => runtime.getStatus().queuedRequestCount === 0)

  const ledger = await readFile(join(rootDir, ".goddard", "ledger.jsonl"), "utf-8")

  expect(ledger).toMatch(/"intent":"create"/)
  expect(capturedSystemPrompt).toMatch(/This request is a create request\./)
  expect(capturedSystemPrompt).toMatch(
    /You are being asked to create a new project from scratch or add new packages to the existing workspace when the requested feature needs them\./,
  )
  expect(capturedInitialPrompt).toMatch(/Request intent: create/)
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
