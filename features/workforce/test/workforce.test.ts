import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join } from "node:path"
import type { DaemonLogService } from "@goddard-ai/daemon-plugin"
import { afterEach, expect, test } from "bun:test"

import { initializeWorkforce } from "../src/daemon/config.ts"
import { WorkforceActorContext, WorkforceDispatchContext } from "../src/daemon/context.ts"
import { createWorkforceManager } from "../src/daemon/manager.ts"
import { normalizeWorkforceRootDir } from "../src/daemon/paths.ts"
import { WorkforceRuntime, type WorkforceRuntimeDeps } from "../src/daemon/runtime.ts"

const cleanup: Array<() => Promise<void>> = []
const ulidPattern = /^[0-9A-HJKMNP-TV-Z]{26}$/
const originalPath = process.env.PATH
type WorkforceNewSessionInput = Parameters<WorkforceRuntimeDeps["session"]["newSession"]>[0]

afterEach(async () => {
  while (cleanup.length > 0) {
    await cleanup.pop()?.()
  }

  if (originalPath === undefined) {
    delete process.env.PATH
  } else {
    process.env.PATH = originalPath
  }
})

function createTestSession() {
  return {
    newSession: async () => ({
      id: "ses_test" as const,
      acpSessionId: "acp_test",
      status: "completed",
    }),
  }
}

test("workforce initialization rejects when no default agent can be resolved", async () => {
  process.env.PATH = ""
  const repoDir = await mkdtemp(join(tmpdir(), "goddard-workforce-missing-agent-"))
  cleanup.push(() => rm(repoDir, { recursive: true, force: true }))

  await writeFile(
    join(repoDir, "package.json"),
    JSON.stringify({ name: "@repo/root", private: true }, null, 2),
    "utf-8",
  )

  await expect(initializeWorkforce(repoDir, [repoDir])).rejects.toThrow(
    "No default ACP agent is configured or discoverable.",
  )
})

test("workforce manager reuses one runtime per normalized repository root", async () => {
  const created: string[] = []
  const manager = createWorkforceManager({
    log: createTestLogService([]),
    session: createTestSession(),
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
    log: createTestLogService([]),
    session: createTestSession(),
    runSession: async ({ request }) => {
      callCount += 1

      if (request.input === "suspend me") {
        await runtime.suspend({
          requestId: request.id,
          reason: "Need a root decision.",
          actor: {
            sessionId: "ses_1",
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
          sessionId: "ses_1",
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
    log: createTestLogService([]),
    session: createTestSession(),
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
  const ledgerEvents = ledger
    .trim()
    .split("\n")
    .map((line) => JSON.parse(line) as { id: string })

  expect(requestId).toMatch(ulidPattern)
  for (const event of ledgerEvents) {
    expect(event.id).toMatch(ulidPattern)
  }
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
  const attachments = new Map<string, { agentId: string; requestId: string }>()

  runtime = await WorkforceRuntime.start(rootDir, {
    log: createTestLogService([]),
    session: {
      newSession: async ({ request: input, onPersisted }: WorkforceNewSessionInput) => {
        expect(input).not.toHaveProperty("workforce")
        await onPersisted?.({ sessionId: "ses_1" })
        const metadata = attachments.get("ses_1") ?? null

        if (!metadata?.agentId || !metadata.requestId) {
          throw new Error("Missing workforce metadata")
        }

        systemPrompts[metadata.agentId] = input.systemPrompt ?? ""

        await runtime.respond({
          requestId: metadata.requestId,
          output: "ok",
          actor: {
            sessionId: "ses_1",
            rootDir: null,
            agentId: metadata.agentId,
            requestId: metadata.requestId,
          },
        })

        return {} as never
      },
    } as never,
    attachSession: (input) => {
      attachments.set(input.sessionId, input)
    },
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
  const attachments = new Map<string, { agentId: string; requestId: string }>()

  runtime = await WorkforceRuntime.start(rootDir, {
    log: createTestLogService([]),
    session: {
      newSession: async ({ request: input, onPersisted }: WorkforceNewSessionInput) => {
        const initialPrompt =
          typeof input.initialPrompt === "string"
            ? input.initialPrompt
            : JSON.stringify(input.initialPrompt)
        capturedEnv = input.env

        if (initialPrompt.includes("Request intent: create")) {
          createSystemPrompt = input.systemPrompt ?? ""
          createInitialPrompt = initialPrompt
        } else {
          defaultSystemPrompt = input.systemPrompt ?? ""
        }

        await onPersisted?.({ sessionId: "ses_1" })
        const metadata = attachments.get("ses_1") ?? null

        if (!metadata?.agentId || !metadata.requestId) {
          throw new Error("Missing workforce metadata")
        }

        await runtime.respond({
          requestId: metadata.requestId,
          output: "created",
          actor: {
            sessionId: "ses_1",
            rootDir: null,
            agentId: metadata.agentId,
            requestId: metadata.requestId,
          },
        })

        return {} as never
      },
    } as never,
    attachSession: (input) => {
      attachments.set(input.sessionId, input)
    },
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
  const attachments = new Map<string, { agentId: string; requestId: string }>()

  runtime = await WorkforceRuntime.start(rootDir, {
    log: createTestLogService([]),
    session: {
      newSession: async ({ request: input, onPersisted }: WorkforceNewSessionInput) => {
        capturedSystemPrompt = input.systemPrompt ?? ""

        await onPersisted?.({ sessionId: "ses_1" })
        const metadata = attachments.get("ses_1") ?? null

        if (!metadata?.agentId || !metadata.requestId) {
          throw new Error("Missing workforce metadata")
        }

        await runtime.respond({
          requestId: metadata.requestId,
          output: "done",
          actor: {
            sessionId: "ses_1",
            rootDir: null,
            agentId: metadata.agentId,
            requestId: metadata.requestId,
          },
        })

        return {} as never
      },
    } as never,
    attachSession: (input) => {
      attachments.set(input.sessionId, input)
    },
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
  const attachments = new Map<string, { agentId: string; requestId: string }>()
  const { logs } = await captureLogs(async (log) => {
    runtime = await WorkforceRuntime.start(rootDir, {
      log,
      session: {
        newSession: async ({ onPersisted }: WorkforceNewSessionInput) => {
          await onPersisted?.({ sessionId: "ses_daemon_1" })
          const metadata = attachments.get("ses_daemon_1") ?? null

          if (!metadata?.agentId || !metadata.requestId) {
            throw new Error("Missing workforce metadata")
          }

          await runtime.respond({
            requestId: metadata.requestId,
            output: "done",
            actor: {
              sessionId: "ses_daemon_1",
              rootDir: null,
              agentId: metadata.agentId,
              requestId: metadata.requestId,
            },
          })

          return {
            id: "ses_daemon_1",
            acpSessionId: "acp-session-1",
            status: "done",
          } as never
        },
      } as never,
      attachSession: (input) => {
        attachments.set(input.sessionId, input)
      },
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
    (entry) => entry.event === "workforce.session_completed" && entry.sessionId === "ses_daemon_1",
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
    "ses_daemon_1",
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
    log: createTestLogService([]),
    session: createTestSession(),
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
        sessionId: "ses_1",
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
        sessionId: "ses_1",
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

function createTestLogService(
  output: Array<Record<string, unknown>>,
  readContext: () => Record<string, unknown> = () => ({}),
): DaemonLogService {
  const logger = {
    log(event: string, fields: Record<string, unknown> = {}) {
      output.push({
        event,
        ...readContext(),
        ...fields,
      })
    },
    snapshot() {
      return logger
    },
  }

  return {
    createLogger: () => logger,
    isVerboseLogging: () => false,
    createPayloadPreview: (value) => value,
    createChunkPreview: (value) => ({
      text: new TextDecoder().decode(value),
      byteLength: value.byteLength,
      truncated: false,
    }),
  }
}

async function captureLogs<T>(
  action: (log: DaemonLogService) => Promise<T>,
): Promise<{ logs: Array<Record<string, unknown>>; result: T }> {
  const logs: Array<Record<string, unknown>> = []
  const result = await action(
    createTestLogService(logs, () => ({
      workforceActor: WorkforceActorContext.get(),
      workforceDispatch: WorkforceDispatchContext.get(),
    })),
  )
  return {
    logs,
    result,
  }
}
