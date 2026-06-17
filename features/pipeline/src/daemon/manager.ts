import { IpcClientError } from "@goddard-ai/ipc"
import type { KindInput } from "kindstore"

import type { PipelineDb } from "../daemon.ts"
import type {
  DaemonPipelineRun,
  DaemonPipelineStepRun,
  PipelineDefinition,
  PipelineRunId,
  PipelineRunStatus,
  PipelineRunWithSteps,
  PipelineScriptStepDefinition,
  PipelineStepRunStatus,
  SpawnPipelineRunRequest,
} from "../schema.ts"
import type { PipelineDefinitionRegistry } from "./registry.ts"

type PipelineRunInput = KindInput<PipelineDb["schema"]["pipelineRuns"]>
type PipelineStepRunInput = KindInput<PipelineDb["schema"]["pipelineStepRuns"]>
type PipelineStepRunUpdate = Partial<PipelineStepRunInput>
type PipelineRunUpdate = Partial<PipelineRunInput>

export type PipelineScriptTransformer = (input: {
  input: Record<string, unknown>
  run: DaemonPipelineRun
  step: PipelineScriptStepDefinition
}) => unknown | Promise<unknown>

type PipelineEventPayloads = {
  "pipeline.run.updated": { runId: string; status: string }
  "pipeline.step.updated": { runId: string; stepId: string; status: string }
}

type PipelineEvent =
  | { name: "pipeline.run.updated"; payload: PipelineEventPayloads["pipeline.run.updated"] }
  | { name: "pipeline.step.updated"; payload: PipelineEventPayloads["pipeline.step.updated"] }

type PipelineEventPublisher = (event: PipelineEvent) => void

const DEFAULT_RUN_PAGE_SIZE = 50
const cancellableStatuses = new Set<PipelineRunStatus>(["queued", "running", "waiting"])
const terminalStepStatuses = new Set<PipelineStepRunStatus>(["succeeded", "failed", "skipped"])

export type PipelineRunManagerInput = {
  db: PipelineDb
  registry: PipelineDefinitionRegistry
  transformers?: Record<string, PipelineScriptTransformer>
  publishEvent?: PipelineEventPublisher
  now?: () => number
}

export function createPipelineRunManager(input: PipelineRunManagerInput) {
  const now = input.now ?? Date.now

  return {
    async spawnRun(request: SpawnPipelineRunRequest): Promise<PipelineRunWithSteps> {
      const registered = await resolveRegisteredDefinition(input.registry, request)
      validatePipelineInputs(registered.definition, request.inputs)

      const timestamp = now()
      let run: DaemonPipelineRun | null = null
      const stepRuns: DaemonPipelineStepRun[] = []

      input.db.batch(() => {
        const runInput: PipelineRunInput = {
          pipelineId: registered.definition.id,
          pipelineVersion: registered.definition.version,
          status: "queued",
          origin: request.origin,
          visibility: request.visibility,
          inputs: request.inputs,
          outputs: null,
          error: null,
          definitionSnapshot: {
            definition: registered.definition,
            source: registered.source,
            path: registered.path,
          },
          startedAt: null,
          completedAt: null,
          updatedAt: timestamp,
        }

        run = input.db.pipelineRuns.create(runInput)

        for (const [stepIndex, step] of registered.definition.steps.entries()) {
          const stepRunInput: PipelineStepRunInput = {
            pipelineRunId: run.id,
            stepId: step.id,
            stepIndex,
            kind: step.kind,
            status: "queued",
            input: {},
            output: null,
            error: null,
            startedAt: null,
            completedAt: null,
            updatedAt: timestamp,
          }
          stepRuns.push(input.db.pipelineStepRuns.create(stepRunInput))
        }
      })

      if (!run) {
        throw new Error("Pipeline run creation failed")
      }

      return { run, steps: stepRuns }
    },

    getRun(id: PipelineRunId): PipelineRunWithSteps {
      const run = input.db.pipelineRuns.get(id)
      if (!run) {
        throw new IpcClientError("Pipeline run not found")
      }

      return {
        run,
        steps: listStepRuns(input.db, id),
      }
    },

    listRuns(params: { pipelineId?: string; limit?: number; cursor?: string | null }) {
      const pageSize = normalizePageSize(params.limit)
      let page: ReturnType<typeof input.db.pipelineRuns.findPage>

      try {
        page = input.db.pipelineRuns.findPage({
          where: params.pipelineId ? { pipelineId: params.pipelineId } : undefined,
          orderBy: {
            createdAt: "desc",
            id: "desc",
          },
          limit: pageSize,
          after: params.cursor ?? undefined,
        })
      } catch {
        throw new IpcClientError("Invalid pipeline run cursor")
      }

      return {
        runs: page.items,
        nextCursor: page.next ?? null,
        hasMore: page.next != null,
      }
    },

    cancelRun(id: PipelineRunId): PipelineRunWithSteps {
      const current = input.db.pipelineRuns.get(id)
      if (!current) {
        throw new IpcClientError("Pipeline run not found")
      }

      if (!cancellableStatuses.has(current.status)) {
        throw new IpcClientError(`Pipeline run cannot be cancelled from ${current.status}`)
      }

      const timestamp = now()
      let run: DaemonPipelineRun | null = null

      input.db.batch(() => {
        run =
          input.db.pipelineRuns.update(id, {
            status: "cancelled",
            completedAt: timestamp,
            updatedAt: timestamp,
          }) ?? null

        for (const stepRun of listStepRuns(input.db, id)) {
          if (terminalStepStatuses.has(stepRun.status)) {
            continue
          }

          input.db.pipelineStepRuns.update(stepRun.id, {
            status: "skipped",
            completedAt: timestamp,
            updatedAt: timestamp,
          })
        }
      })

      if (!run) {
        throw new IpcClientError("Pipeline run not found")
      }

      return {
        run,
        steps: listStepRuns(input.db, id),
      }
    },

    async advanceRun(id: PipelineRunId): Promise<PipelineRunWithSteps> {
      let current = this.getRun(id)
      if (isTerminalRunStatus(current.run.status)) {
        return current
      }

      if (current.run.status === "failed") {
        throw new IpcClientError("Failed pipeline runs must be retried")
      }

      while (true) {
        current = this.getRun(id)

        if (current.run.status === "cancelled") {
          return current
        }

        const nextStepRun = current.steps.find((step) => step.status === "queued")
        if (!nextStepRun) {
          return updateRun(input, id, {
            status: "succeeded",
            outputs: collectRunOutputs(current.steps),
            completedAt: now(),
            updatedAt: now(),
          })
        }

        const stepDefinition =
          current.run.definitionSnapshot.definition.steps[nextStepRun.stepIndex]
        if (!stepDefinition) {
          return failRun(
            input,
            id,
            nextStepRun,
            `Step definition missing at index ${nextStepRun.stepIndex}`,
          )
        }

        const mappedInput = mapStepInput(current.run, current.steps, stepDefinition.input)

        if (stepDefinition.kind === "approval") {
          input.db.batch(() => {
            updateStep(input, nextStepRun.id, {
              status: "waiting",
              input: mappedInput,
              startedAt: now(),
              updatedAt: now(),
            })
            updateRun(input, id, {
              status: "waiting",
              startedAt: current.run.startedAt ?? now(),
              updatedAt: now(),
            })
          })
          return this.getRun(id)
        }

        if (stepDefinition.kind === "agent") {
          input.db.batch(() => {
            updateStep(input, nextStepRun.id, {
              status: "waiting",
              input: mappedInput,
              startedAt: now(),
              updatedAt: now(),
            })
            updateRun(input, id, {
              status: "waiting",
              startedAt: current.run.startedAt ?? now(),
              updatedAt: now(),
            })
          })
          return this.getRun(id)
        }

        const transformer = input.transformers?.[stepDefinition.transformer]
        if (!transformer) {
          return failRun(
            input,
            id,
            nextStepRun,
            `Pipeline script transformer not registered: ${stepDefinition.transformer}`,
            mappedInput,
          )
        }

        updateRun(input, id, {
          status: "running",
          startedAt: current.run.startedAt ?? now(),
          updatedAt: now(),
        })
        updateStep(input, nextStepRun.id, {
          status: "running",
          input: mappedInput,
          startedAt: now(),
          updatedAt: now(),
        })

        try {
          const output = await transformer({
            input: mappedInput,
            run: this.getRun(id).run,
            step: stepDefinition,
          })
          updateStep(input, nextStepRun.id, {
            status: "succeeded",
            output,
            error: null,
            completedAt: now(),
            updatedAt: now(),
          })
        } catch (error) {
          return failRun(input, id, nextStepRun, getErrorMessage(error), mappedInput)
        }
      }
    },

    async approveRun(id: PipelineRunId): Promise<PipelineRunWithSteps> {
      const current = this.getRun(id)
      const approvalStep = current.steps.find(
        (step) => step.kind === "approval" && step.status === "waiting",
      )
      if (!approvalStep) {
        throw new IpcClientError("Pipeline run is not waiting for approval")
      }

      updateStep(input, approvalStep.id, {
        status: "succeeded",
        output: { approved: true },
        completedAt: now(),
        updatedAt: now(),
      })
      updateRun(input, id, {
        status: "queued",
        updatedAt: now(),
      })

      return this.advanceRun(id)
    },

    async retryRun(id: PipelineRunId): Promise<PipelineRunWithSteps> {
      const current = this.getRun(id)
      if (current.run.status !== "failed") {
        throw new IpcClientError("Only failed pipeline runs can be retried")
      }

      const failedStep = current.steps.find((step) => step.status === "failed")
      if (!failedStep) {
        throw new IpcClientError("Pipeline run has no failed step to retry")
      }

      input.db.batch(() => {
        for (const stepRun of current.steps) {
          if (stepRun.stepIndex < failedStep.stepIndex) {
            continue
          }

          updateStep(input, stepRun.id, {
            status: "queued",
            input: {},
            output: null,
            error: null,
            startedAt: null,
            completedAt: null,
            updatedAt: now(),
          })
        }

        updateRun(input, id, {
          status: "queued",
          error: null,
          completedAt: null,
          updatedAt: now(),
        })
      })

      return this.advanceRun(id)
    },
  }
}

async function resolveRegisteredDefinition(
  registry: PipelineDefinitionRegistry,
  request: SpawnPipelineRunRequest,
) {
  const result = await registry.list({ cwd: request.cwd })
  const matches = result.definitions.filter(
    (item) =>
      item.definition.id === request.pipelineId &&
      (!request.pipelineVersion || item.definition.version === request.pipelineVersion),
  )

  if (matches.length === 0) {
    throw new IpcClientError("Pipeline definition not found")
  }

  if (!request.pipelineVersion && matches.length > 1) {
    throw new IpcClientError("Pipeline version is required when multiple versions exist")
  }

  return matches[0]
}

function validatePipelineInputs(definition: PipelineDefinition, inputs: Record<string, unknown>) {
  const expectedKeys = new Set(Object.keys(definition.inputs))
  const actualKeys = new Set(Object.keys(inputs))
  const missingKeys = [...expectedKeys].filter((key) => !actualKeys.has(key))
  const unknownKeys = [...actualKeys].filter((key) => !expectedKeys.has(key))

  if (missingKeys.length > 0 || unknownKeys.length > 0) {
    throw new IpcClientError(
      [
        missingKeys.length > 0 ? `missing inputs: ${missingKeys.join(", ")}` : null,
        unknownKeys.length > 0 ? `unknown inputs: ${unknownKeys.join(", ")}` : null,
      ]
        .filter(Boolean)
        .join("; "),
    )
  }
}

function listStepRuns(db: PipelineDb, pipelineRunId: PipelineRunId) {
  return db.pipelineStepRuns
    .findMany({
      where: { pipelineRunId },
    })
    .sort((left, right) => left.stepIndex - right.stepIndex)
}

function normalizePageSize(limit?: number) {
  if (!Number.isFinite(limit)) {
    return DEFAULT_RUN_PAGE_SIZE
  }

  return Math.min(Math.max(Math.trunc(limit ?? DEFAULT_RUN_PAGE_SIZE), 1), 100)
}

function updateRun(input: PipelineRunManagerInput, id: PipelineRunId, update: PipelineRunUpdate) {
  const run = input.db.pipelineRuns.update(id, update)
  if (!run) {
    throw new IpcClientError("Pipeline run not found")
  }

  input.publishEvent?.({
    name: "pipeline.run.updated",
    payload: {
      runId: run.id,
      status: run.status,
    },
  })

  return {
    run,
    steps: listStepRuns(input.db, id),
  }
}

function updateStep(
  input: PipelineRunManagerInput,
  id: DaemonPipelineStepRun["id"],
  update: PipelineStepRunUpdate,
) {
  const step = input.db.pipelineStepRuns.update(id, update)
  if (!step) {
    throw new IpcClientError("Pipeline step run not found")
  }

  input.publishEvent?.({
    name: "pipeline.step.updated",
    payload: {
      runId: step.pipelineRunId,
      stepId: step.stepId,
      status: step.status,
    },
  })

  return step
}

function failRun(
  input: PipelineRunManagerInput,
  runId: PipelineRunId,
  stepRun: DaemonPipelineStepRun,
  message: string,
  mappedInput?: Record<string, unknown>,
) {
  const timestamp = input.now?.() ?? Date.now()

  input.db.batch(() => {
    updateStep(input, stepRun.id, {
      status: "failed",
      ...(mappedInput && { input: mappedInput }),
      error: message,
      completedAt: timestamp,
      updatedAt: timestamp,
    })
    updateRun(input, runId, {
      status: "failed",
      error: message,
      completedAt: timestamp,
      updatedAt: timestamp,
    })
  })

  return {
    run: input.db.pipelineRuns.get(runId)!,
    steps: listStepRuns(input.db, runId),
  }
}

function mapStepInput(
  run: DaemonPipelineRun,
  steps: readonly DaemonPipelineStepRun[],
  mapping: Record<string, string>,
) {
  const output: Record<string, unknown> = {}

  for (const [key, reference] of Object.entries(mapping)) {
    output[key] = resolveStepReference(run, steps, reference)
  }

  return output
}

function resolveStepReference(
  run: DaemonPipelineRun,
  steps: readonly DaemonPipelineStepRun[],
  reference: string,
) {
  if (reference.startsWith("$.inputs.")) {
    return getPathValue(run.inputs, reference.slice("$.inputs.".length))
  }

  if (!reference.startsWith("$.steps.")) {
    throw new IpcClientError(`Unsupported pipeline input reference: ${reference}`)
  }

  const path = reference.slice("$.steps.".length)
  const [stepId, outputKey, ...rest] = path.split(".")
  if (outputKey !== "output") {
    throw new IpcClientError(`Unsupported pipeline step reference: ${reference}`)
  }

  const step = steps.find((item) => item.stepId === stepId)
  if (!step || step.status !== "succeeded") {
    throw new IpcClientError(`Pipeline step output is not available: ${stepId}`)
  }

  return rest.length > 0 ? getPathValue(step.output, rest.join(".")) : step.output
}

function getPathValue(value: unknown, path: string) {
  if (path.length === 0) {
    return value
  }

  let current = value
  for (const segment of path.split(".")) {
    if (typeof current !== "object" || current === null || !(segment in current)) {
      return undefined
    }
    current = (current as Record<string, unknown>)[segment]
  }

  return current
}

function collectRunOutputs(steps: readonly DaemonPipelineStepRun[]) {
  return {
    steps: Object.fromEntries(steps.map((step) => [step.stepId, step.output])),
  }
}

function isTerminalRunStatus(status: PipelineRunStatus) {
  return status === "succeeded" || status === "cancelled"
}

function getErrorMessage(error: unknown) {
  return error instanceof Error ? error.message : String(error)
}
