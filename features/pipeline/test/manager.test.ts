import { afterEach, beforeEach, expect, test } from "bun:test"
import { kindstore, type Kindstore } from "kindstore"

import {
  createPipelineRunManager,
  pipelinePlugin,
  type PipelineScriptTransformer,
  type PipelineSessionService,
} from "../src/daemon.ts"
import type { PipelineDefinitionRegistry } from "../src/daemon/registry.ts"
import {
  SpawnPipelineRunRequest,
  type PipelineDefinition,
  type RegisteredPipelineDefinition,
} from "../src/schema.ts"

let store: Kindstore<(typeof pipelinePlugin)["db"]["schema"], {}>

beforeEach(() => {
  store = kindstore({
    filename: ":memory:",
    schema: pipelinePlugin.db.schema,
  })
})

afterEach(() => {
  store.close()
})

function definition(version = "0.1.0", steps?: PipelineDefinition["steps"]): PipelineDefinition {
  return {
    id: "creative-weaver",
    version,
    name: "Creative Weaver",
    inputs: {
      premise: { type: "string" },
    },
    steps: steps ?? [
      {
        id: "architect",
        kind: "script",
        name: "Architect",
        transformer: "creative-weaver.architect",
        input: {
          premise: "$.inputs.premise",
        },
      },
      {
        id: "chaos",
        kind: "script",
        name: "Chaos Weaver",
        transformer: "creative-weaver.sample-chaos",
        input: {
          ledger: "$.steps.architect.output.ledger",
        },
      },
    ],
  }
}

function registry(...definitions: PipelineDefinition[]): PipelineDefinitionRegistry {
  const registeredDefinitions: RegisteredPipelineDefinition[] = definitions.map((item) => ({
    source: "code",
    definition: item,
  }))

  return {
    async list() {
      return {
        definitions: registeredDefinitions,
        diagnostics: [],
      }
    },
    async diagnostics() {
      return {
        diagnostics: [],
      }
    },
  }
}

function createManager(
  input: {
    definitions?: PipelineDefinition[]
    now?: () => number
    session?: PipelineSessionService
    transformers?: Record<string, PipelineScriptTransformer>
  } = {},
) {
  return createPipelineRunManager({
    db: store,
    registry: registry(...(input.definitions ?? [definition()])),
    session: input.session,
    transformers: input.transformers,
    now: input.now,
  })
}

test("spawnRun persists a queued run with ordered queued step runs", async () => {
  const manager = createManager({ now: () => 1000 })

  const result = await manager.spawnRun(
    SpawnPipelineRunRequest.parse({
      cwd: "/repo",
      pipelineId: "creative-weaver",
      pipelineVersion: "0.1.0",
      inputs: {
        premise: "A haunted lighthouse",
      },
      origin: "app",
      visibility: "visible",
    }),
  )

  expect(result.run).toMatchObject({
    pipelineId: "creative-weaver",
    pipelineVersion: "0.1.0",
    status: "queued",
    origin: "app",
    visibility: "visible",
    inputs: {
      premise: "A haunted lighthouse",
    },
    outputs: null,
    error: null,
    definitionSnapshot: {
      source: "code",
      definition: {
        id: "creative-weaver",
        version: "0.1.0",
      },
    },
    startedAt: null,
    completedAt: null,
    updatedAt: 1000,
  })
  expect(result.steps.map((step) => [step.stepIndex, step.stepId, step.kind, step.status])).toEqual(
    [
      [0, "architect", "script", "queued"],
      [1, "chaos", "script", "queued"],
    ],
  )
  expect(store.pipelineRuns.findMany()).toHaveLength(1)
  expect(store.pipelineStepRuns.findMany({ where: { pipelineRunId: result.run.id } })).toHaveLength(
    2,
  )
})

test("getRun returns the persisted schema snapshot and ordered steps", async () => {
  const manager = createManager()
  const created = await manager.spawnRun(
    SpawnPipelineRunRequest.parse({
      cwd: "/repo",
      pipelineId: "creative-weaver",
      inputs: {
        premise: "A quiet breakup",
      },
    }),
  )

  const result = manager.getRun(created.run.id)

  expect(result.run.definitionSnapshot.definition.steps.map((step) => step.id)).toEqual([
    "architect",
    "chaos",
  ])
  expect(result.steps.map((step) => step.stepId)).toEqual(["architect", "chaos"])
})

test("listRuns filters runs and returns daemon ordering", async () => {
  const manager = createPipelineRunManager({
    db: store,
    registry: registry(definition("0.1.0"), { ...definition("0.2.0"), id: "revision-loop" }),
    now: () => 1000,
  })
  await manager.spawnRun(
    SpawnPipelineRunRequest.parse({
      cwd: "/repo",
      pipelineId: "creative-weaver",
      pipelineVersion: "0.1.0",
      inputs: { premise: "One" },
    }),
  )
  await manager.spawnRun(
    SpawnPipelineRunRequest.parse({
      cwd: "/repo",
      pipelineId: "revision-loop",
      pipelineVersion: "0.2.0",
      inputs: { premise: "Two" },
    }),
  )

  const result = manager.listRuns({ pipelineId: "creative-weaver" })

  expect(result.hasMore).toBe(false)
  expect(result.nextCursor).toBe(null)
  expect(result.runs.map((run) => run.pipelineId)).toEqual(["creative-weaver"])
})

test("cancelRun marks queued runs cancelled and unstarted steps skipped", async () => {
  let time = 1000
  const manager = createManager({ now: () => time })
  const created = await manager.spawnRun(
    SpawnPipelineRunRequest.parse({
      cwd: "/repo",
      pipelineId: "creative-weaver",
      inputs: { premise: "Cancel me" },
    }),
  )

  time = 2000
  const result = await manager.cancelRun(created.run.id)

  expect(result.run).toMatchObject({
    status: "cancelled",
    completedAt: 2000,
    updatedAt: 2000,
  })
  expect(result.steps.map((step) => step.status)).toEqual(["skipped", "skipped"])
  expect(new Set(result.steps.map((step) => step.completedAt))).toEqual(new Set([2000]))
})

test("spawnRun validates declared input keys", async () => {
  const manager = createManager()

  await expect(
    manager.spawnRun(
      SpawnPipelineRunRequest.parse({
        cwd: "/repo",
        pipelineId: "creative-weaver",
        inputs: {
          extra: "surprise",
        },
      }),
    ),
  ).rejects.toThrow(/missing inputs: premise; unknown inputs: extra/)
})

test("advanceRun executes script steps and maps prior outputs", async () => {
  const manager = createManager({
    transformers: {
      "creative-weaver.architect": ({ input }) => ({ ledger: `Scene: ${input.premise}` }),
      "creative-weaver.sample-chaos": ({ input }) => ({ prose: `${input.ledger} / salt air` }),
    },
  })
  const created = await manager.spawnRun(
    SpawnPipelineRunRequest.parse({
      cwd: "/repo",
      pipelineId: "creative-weaver",
      inputs: { premise: "A lighthouse" },
    }),
  )

  const result = await manager.advanceRun(created.run.id)

  expect(result.run).toMatchObject({
    status: "succeeded",
    outputs: {
      steps: {
        architect: { ledger: "Scene: A lighthouse" },
        chaos: { prose: "Scene: A lighthouse / salt air" },
      },
    },
  })
  expect(result.steps.map((step) => step.status)).toEqual(["succeeded", "succeeded"])
  expect(result.steps[1]?.input).toEqual({ ledger: "Scene: A lighthouse" })
})

test("advanceRun fails a script step when its transformer is not registered", async () => {
  const manager = createManager()
  const created = await manager.spawnRun(
    SpawnPipelineRunRequest.parse({
      cwd: "/repo",
      pipelineId: "creative-weaver",
      inputs: { premise: "A lighthouse" },
    }),
  )

  const result = await manager.advanceRun(created.run.id)

  expect(result.run).toMatchObject({
    status: "failed",
    error: "Pipeline script transformer not registered: creative-weaver.architect",
  })
  expect(result.steps.map((step) => step.status)).toEqual(["failed", "queued"])
})

test("approval steps pause and resume without losing prior outputs", async () => {
  const approvalDefinition = definition("0.1.0", [
    {
      id: "draft",
      kind: "script",
      name: "Draft",
      transformer: "draft",
      input: { premise: "$.inputs.premise" },
    },
    {
      id: "approval",
      kind: "approval",
      name: "Approve",
      input: { draft: "$.steps.draft.output" },
    },
    {
      id: "polish",
      kind: "script",
      name: "Polish",
      transformer: "polish",
      input: { draft: "$.steps.draft.output.text" },
    },
  ])
  const manager = createManager({
    definitions: [approvalDefinition],
    transformers: {
      draft: ({ input }) => ({ text: String(input.premise).toUpperCase() }),
      polish: ({ input }) => ({ text: `${input.draft}!` }),
    },
  })
  const created = await manager.spawnRun(
    SpawnPipelineRunRequest.parse({
      cwd: "/repo",
      pipelineId: "creative-weaver",
      inputs: { premise: "quiet room" },
    }),
  )

  const waiting = await manager.advanceRun(created.run.id)
  expect(waiting.run.status).toBe("waiting")
  expect(waiting.steps.map((step) => step.status)).toEqual(["succeeded", "waiting", "queued"])

  const completed = await manager.approveRun(created.run.id)
  expect(completed.run.status).toBe("succeeded")
  expect(completed.steps.map((step) => step.status)).toEqual([
    "succeeded",
    "succeeded",
    "succeeded",
  ])
  expect(completed.steps[0]?.output).toEqual({ text: "QUIET ROOM" })
  expect(completed.steps[1]?.output).toEqual({ approved: true })
  expect(completed.steps[2]?.input).toEqual({ draft: "QUIET ROOM" })
})

test("retryRun replays the failed script step and leaves prior outputs intact", async () => {
  let attempts = 0
  const manager = createManager({
    transformers: {
      "creative-weaver.architect": () => ({ ledger: "stable" }),
      "creative-weaver.sample-chaos": () => {
        attempts += 1
        if (attempts === 1) {
          throw new Error("temporary failure")
        }
        return { prose: "recovered" }
      },
    },
  })
  const created = await manager.spawnRun(
    SpawnPipelineRunRequest.parse({
      cwd: "/repo",
      pipelineId: "creative-weaver",
      inputs: { premise: "A lighthouse" },
    }),
  )

  const failed = await manager.advanceRun(created.run.id)
  expect(failed.run.status).toBe("failed")
  expect(failed.steps.map((step) => step.status)).toEqual(["succeeded", "failed"])

  const retried = await manager.retryRun(created.run.id)
  expect(retried.run.status).toBe("succeeded")
  expect(retried.steps[0]?.output).toEqual({ ledger: "stable" })
  expect(retried.steps[1]?.output).toEqual({ prose: "recovered" })
})

test("cancelRun prevents queued future steps from executing", async () => {
  let ran = false
  const manager = createManager({
    transformers: {
      "creative-weaver.architect": () => {
        ran = true
        return {}
      },
    },
  })
  const created = await manager.spawnRun(
    SpawnPipelineRunRequest.parse({
      cwd: "/repo",
      pipelineId: "creative-weaver",
      inputs: { premise: "A lighthouse" },
    }),
  )

  await manager.cancelRun(created.run.id)
  const cancelled = await manager.advanceRun(created.run.id)

  expect(cancelled.run.status).toBe("cancelled")
  expect(cancelled.steps.map((step) => step.status)).toEqual(["skipped", "skipped"])
  expect(ran).toBe(false)
})

test("advanceRun executes agent steps through hidden pipeline sessions", async () => {
  const requests: Array<Parameters<PipelineSessionService["newSession"]>[0]["request"]> = []
  const agentDefinition = definition("0.1.0", [
    {
      id: "agent",
      kind: "agent",
      name: "Agent",
      systemPrompt: "Write one paragraph.",
      model: "gpt-test",
      input: { premise: "$.inputs.premise" },
    },
  ])
  const manager = createManager({
    definitions: [agentDefinition],
    session: {
      async newSession({ request }) {
        requests.push(request)
        return {
          id: "ses_pipeline",
          acpSessionId: "acp_pipeline",
          status: "done",
          stopReason: "end_turn",
          agent: null,
          agentName: "Test Agent",
          cwd: request.cwd,
          title: "Pipeline Agent",
          titleState: "placeholder",
          lastSessionActivityAt: 1,
          mcpServers: [],
          connectionMode: "none",
          supportsLoadSession: false,
          activeDaemonSession: false,
          completedHidden: false,
          origin: "pipeline",
          visibility: "hidden",
          errorMessage: null,
          blockedReason: null,
          initiative: null,
          inboxScope: null,
          lastAgentMessage: "finished prose",
          repository: null,
          prNumber: null,
          token: null,
          permissions: null,
          metadata: request.metadata ?? null,
          configOptions: [],
          availableCommands: [],
          contextUsage: null,
          createdAt: 1,
        }
      },
      async shutdownSession() {
        return true
      },
    },
  })
  const created = await manager.spawnRun(
    SpawnPipelineRunRequest.parse({
      cwd: "/repo",
      pipelineId: "creative-weaver",
      inputs: { premise: "winter pier" },
    }),
  )

  const result = await manager.advanceRun(created.run.id)

  expect(result.run.status).toBe("succeeded")
  expect(result.steps[0]).toMatchObject({
    status: "succeeded",
    sessionId: "ses_pipeline",
    stopReason: "end_turn",
    output: {
      sessionId: "ses_pipeline",
      stopReason: "end_turn",
      message: "finished prose",
      status: "done",
    },
  })
  expect(requests[0]).toMatchObject({
    cwd: "/repo",
    systemPrompt: "Write one paragraph.",
    initialModelId: "gpt-test",
    oneShot: true,
    origin: "pipeline",
    visibility: "hidden",
    metadata: {
      pipelineRunId: created.run.id,
      pipelineStepId: "agent",
    },
  })
  expect(requests[0]?.initialPrompt).toContain('"premise": "winter pier"')
})

test("advanceRun records agent step failures", async () => {
  const agentDefinition = definition("0.1.0", [
    {
      id: "agent",
      kind: "agent",
      name: "Agent",
      systemPrompt: "Write one paragraph.",
      input: { premise: "$.inputs.premise" },
    },
  ])
  const manager = createManager({
    definitions: [agentDefinition],
    session: {
      async newSession() {
        throw new Error("agent crashed")
      },
      async shutdownSession() {
        return true
      },
    },
  })
  const created = await manager.spawnRun(
    SpawnPipelineRunRequest.parse({
      cwd: "/repo",
      pipelineId: "creative-weaver",
      inputs: { premise: "winter pier" },
    }),
  )

  const result = await manager.advanceRun(created.run.id)

  expect(result.run).toMatchObject({
    status: "failed",
    error: "agent crashed",
  })
  expect(result.steps[0]).toMatchObject({
    status: "failed",
    error: "agent crashed",
  })
})

test("cancelRun shuts down active agent sessions when possible", async () => {
  const shutdowns: string[] = []
  const manager = createManager({
    session: {
      async newSession() {
        throw new Error("not used")
      },
      async shutdownSession(id) {
        shutdowns.push(id)
        return true
      },
    },
  })
  const created = await manager.spawnRun(
    SpawnPipelineRunRequest.parse({
      cwd: "/repo",
      pipelineId: "creative-weaver",
      inputs: { premise: "winter pier" },
    }),
  )
  const agentStep = store.pipelineStepRuns.create({
    pipelineRunId: created.run.id,
    stepId: "agent",
    stepIndex: 2,
    kind: "agent",
    status: "running",
    input: {},
    output: null,
    error: null,
    sessionId: "ses_active",
    stopReason: null,
    startedAt: 1,
    completedAt: null,
    updatedAt: 1,
  })
  store.pipelineRuns.update(created.run.id, { status: "running" })

  await manager.cancelRun(created.run.id)

  expect(shutdowns).toEqual(["ses_active"])
  expect(store.pipelineStepRuns.get(agentStep.id)?.status).toBe("skipped")
})
