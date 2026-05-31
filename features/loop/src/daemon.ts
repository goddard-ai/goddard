import { definePlugin } from "@goddard-ai/daemon-plugin"
import { sessionPlugin } from "@goddard-ai/session/daemon"

import { loopIpcRoutes } from "./daemon-ipc.ts"
import { createLoopManager } from "./daemon/manager.ts"
import { resolveNamedLoopStartRequest } from "./daemon/resolver.ts"
import { LoopConfig, mergeLoopConfigLayers } from "./schema.ts"

export const loopPlugin = definePlugin({
  name: "loop",
  consumes: [sessionPlugin],
  config: {
    loops: {
      schema: LoopConfig,
      scopes: ["user", "project"],
      resolve: ({ project, user }) => mergeLoopConfigLayers(user, project),
    },
  },
  jsonSchemas: [{ name: "loop.json", schema: LoopConfig }],
  ipcRoutes: loopIpcRoutes,
  setup({ configProvider, session }) {
    const loop = createLoopManager({
      session,
      resolveLoopStartRequest: (input) => resolveNamedLoopStartRequest(input, configProvider),
    })

    return {
      close: () => loop.close(),
      ipcHandlers: {
        loop: {
          start: async ({ body }) => ({
            loop: await loop.startLoop(body),
          }),
          get: async ({ body: { rootDir, loopName } }) => ({
            loop: await loop.getLoop(rootDir, loopName),
          }),
          list: async () => ({
            loops: await loop.listLoops(),
          }),
          shutdown: async ({ body: { rootDir, loopName } }) => ({
            rootDir,
            loopName,
            success: await loop.shutdownLoop(rootDir, loopName),
          }),
        },
      },
    }
  },
})
