import * as assert from "node:assert/strict"
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { afterEach, test } from "vitest"
import { createDaemonIpcClient } from "@goddard-ai/daemon-client"
import { startDaemonServer } from "../src/ipc.ts"
import { createWorkforceManager } from "../src/workforce/manager.ts"
import { WorkforceRuntime } from "../src/workforce/runtime.ts"

const cleanup: Array<() => Promise<void>> = []

afterEach(async () => {
  while (cleanup.length > 0) {
    await cleanup.pop()?.()
  }
})

test("daemon IPC exposes repo-root workforce lifecycle methods", async () => {
  const daemon = await startDaemonServer(
    {
      pr: {
        create: async () => ({ number: 1, url: "https://example.com/pr/1" }),
        reply: async () => ({ success: true }),
      },
    },
    {},
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
  cleanup.push(() => daemon.close())

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

  assert.equal(started.workforce.rootDir, "/repo")
  assert.equal(fetched.workforce.rootDir, "/repo")
  assert.equal(listed.workforces.length, 1)
  assert.equal(requested.requestId, "req-1")
  assert.equal(stopped.success, true)
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

  assert.deepEqual(created, [tempRoot])
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

  assert.match(ledger, /"type":"response"/)
  assert.match(ledger, /"type":"suspend"/)
  assert.match(ledger, /"type":"error"/)
  assert.equal(callCount >= 5, true)
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
