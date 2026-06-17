import { definePlugin, event } from "@goddard-ai/daemon-plugin"
import { sessionPlugin } from "@goddard-ai/session/daemon"
import { kind } from "kindstore"

import { pipelineIpcRoutes } from "./daemon-ipc.ts"
import { createPipelineRunManager } from "./daemon/manager.ts"
import { createPipelineDefinitionRegistry } from "./daemon/registry.ts"
import { DaemonPipelineRun, DaemonPipelineStepRun } from "./schema.ts"

export { createPipelineRunManager } from "./daemon/manager.ts"
export type { PipelineScriptTransformer, PipelineSessionService } from "./daemon/manager.ts"
export { createPipelineDefinitionRegistry } from "./daemon/registry.ts"

const pipelineDb = {
  pipelineRuns: kind("plr", DaemonPipelineRun)
    .createdAt()
    .index("pipelineId", { type: "text" })
    .multi("createdAt_id", {
      createdAt: "desc",
      id: "desc",
    }),
  pipelineStepRuns: kind("pls", DaemonPipelineStepRun)
    .createdAt()
    .index("pipelineRunId", { type: "text" })
    .multi(
      "pipelineRunId_stepIndex",
      {
        pipelineRunId: "asc",
        stepIndex: "asc",
      },
      { unique: true },
    ),
}

export const pipelinePlugin = definePlugin({
  name: "pipeline",
  consumes: [sessionPlugin],
  events: {
    "pipeline.run.updated": event<{ runId: string; status: string }>(),
    "pipeline.step.updated": event<{ runId: string; stepId: string; status: string }>(),
  },
  db: {
    schema: pipelineDb,
  },
  ipcRoutes: pipelineIpcRoutes,
  setup({ db, events, session }) {
    const registry = createPipelineDefinitionRegistry()
    const runs = createPipelineRunManager({
      db,
      registry,
      session,
      publishEvent: (event) => {
        if (event.name === "pipeline.run.updated") {
          void events.emit(event.name, event.payload)
          return
        }

        void events.emit(event.name, event.payload)
      },
    })

    return {
      ipcHandlers: {
        pipeline: {
          listDefinitions: async ({ body }) => registry.list(body),
          listDefinitionDiagnostics: async ({ body }) => registry.diagnostics(body),
          spawnRun: async ({ body }) => runs.spawnRun(body),
          getRun: async ({ body: { id } }) => runs.getRun(id),
          listRuns: async ({ body }) => runs.listRuns(body),
          cancelRun: async ({ body: { id } }) => runs.cancelRun(id),
          advanceRun: async ({ body: { id } }) => runs.advanceRun(id),
          approveRun: async ({ body: { id } }) => runs.approveRun(id),
          retryRun: async ({ body: { id } }) => runs.retryRun(id),
        },
      },
    }
  },
})

export type PipelineDb = import("@goddard-ai/daemon-plugin").DbContext<typeof pipelineDb>
