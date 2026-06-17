import { definePlugin } from "@goddard-ai/daemon-plugin"
import { kind } from "kindstore"

import { pipelineIpcRoutes } from "./daemon-ipc.ts"
import { createPipelineRunManager } from "./daemon/manager.ts"
import { createPipelineDefinitionRegistry } from "./daemon/registry.ts"
import { DaemonPipelineRun, DaemonPipelineStepRun } from "./schema.ts"

export { createPipelineRunManager } from "./daemon/manager.ts"
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
  db: {
    schema: pipelineDb,
  },
  ipcRoutes: pipelineIpcRoutes,
  setup({ db }) {
    const registry = createPipelineDefinitionRegistry()
    const runs = createPipelineRunManager({ db, registry })

    return {
      ipcHandlers: {
        pipeline: {
          listDefinitions: async ({ body }) => registry.list(body),
          listDefinitionDiagnostics: async ({ body }) => registry.diagnostics(body),
          spawnRun: async ({ body }) => runs.spawnRun(body),
          getRun: async ({ body: { id } }) => runs.getRun(id),
          listRuns: async ({ body }) => runs.listRuns(body),
          cancelRun: async ({ body: { id } }) => runs.cancelRun(id),
        },
      },
    }
  },
})

export type PipelineDb = import("@goddard-ai/daemon-plugin").DbContext<typeof pipelineDb>
