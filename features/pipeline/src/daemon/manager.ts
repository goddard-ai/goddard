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
  PipelineStepRunStatus,
  SpawnPipelineRunRequest,
} from "../schema.ts"
import type { PipelineDefinitionRegistry } from "./registry.ts"

type PipelineRunInput = KindInput<PipelineDb["schema"]["pipelineRuns"]>
type PipelineStepRunInput = KindInput<PipelineDb["schema"]["pipelineStepRuns"]>

const DEFAULT_RUN_PAGE_SIZE = 50
const cancellableStatuses = new Set<PipelineRunStatus>(["queued", "running", "waiting"])
const terminalStepStatuses = new Set<PipelineStepRunStatus>(["succeeded", "failed", "skipped"])

export type PipelineRunManagerInput = {
  db: PipelineDb
  registry: PipelineDefinitionRegistry
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
