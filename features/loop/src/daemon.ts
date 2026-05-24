import { definePlugin } from "@goddard-ai/daemon-plugin"
import { sessionPlugin } from "@goddard-ai/session/daemon"

import { loopIpcRoutes } from "./daemon-ipc.ts"
import { createLoopManager } from "./daemon/manager.ts"
import { resolveNamedLoopStartRequest } from "./daemon/resolver.ts"
import { LoopConfig } from "./schema.ts"

export const loopPlugin = definePlugin({
  name: "loop",
  consumes: [sessionPlugin],
  config: {
    schema: LoopConfig,
    scopes: ["user", "project"],
  },
  ipcRoutes: loopIpcRoutes,
  setup({ configManager, session }) {
    const loop = createLoopManager({
      session,
      resolveLoopStartRequest: (input) => resolveNamedLoopStartRequest(input, configManager),
    })

    return {
      close: () => loop.close(),
      routeHandlers: {
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
