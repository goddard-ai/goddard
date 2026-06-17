import { definePlugin } from "@goddard-ai/daemon-plugin"

import { pipelineIpcRoutes } from "./daemon-ipc.ts"
import { createPipelineDefinitionRegistry } from "./daemon/registry.ts"

export { createPipelineDefinitionRegistry } from "./daemon/registry.ts"

export const pipelinePlugin = definePlugin({
  name: "pipeline",
  ipcRoutes: pipelineIpcRoutes,
  setup() {
    const registry = createPipelineDefinitionRegistry()

    return {
      ipcHandlers: {
        pipeline: {
          listDefinitions: async ({ body }) => registry.list(body),
          listDefinitionDiagnostics: async ({ body }) => registry.diagnostics(body),
        },
      },
    }
  },
})
