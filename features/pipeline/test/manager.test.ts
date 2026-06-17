import { afterEach, beforeEach, expect, test } from "bun:test"
import { kindstore, type Kindstore } from "kindstore"

import { createPipelineRunManager, pipelinePlugin } from "../src/daemon.ts"
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

function definition(version = "0.1.0"): PipelineDefinition {
  return {
    id: "creative-weaver",
    version,
    name: "Creative Weaver",
    inputs: {
      premise: { type: "string" },
    },
    steps: [
      {
        id: "architect",
        kind: "agent",
        name: "Architect",
        systemPrompt: "Create a ledger.",
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
          ledger: "$.steps.architect.output",
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

function createManager(input: { definitions?: PipelineDefinition[]; now?: () => number } = {}) {
  return createPipelineRunManager({
    db: store,
    registry: registry(...(input.definitions ?? [definition()])),
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
      [0, "architect", "agent", "queued"],
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
  const result = manager.cancelRun(created.run.id)

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
