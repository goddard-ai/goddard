import { definePlugin, type DbContext } from "@goddard-ai/daemon-plugin"
import { sessionPlugin } from "@goddard-ai/session/daemon"
import { kind } from "kindstore"

import { loopIpcRoutes } from "./daemon-ipc.ts"
import { LoopContext } from "./daemon/context.ts"
import { createLoopManager } from "./daemon/manager.ts"
import { resolveNamedLoopStartRequest } from "./daemon/resolver.ts"
import { DaemonLoopSession, LoopConfig, mergeLoopConfigLayers } from "./schema.ts"

const loopDb = {
  loopSessions: kind("lop", DaemonLoopSession)
    .index("sessionId", { type: "text" })
    .multi("rootDir_loopName", {
      rootDir: "asc",
      loopName: "asc",
    }),
}

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
  db: loopDb,
  jsonSchemas: [{ name: "loop.json", schema: LoopConfig }],
  ipcRoutes: loopIpcRoutes,
  logContext: {
    read: () => ({ loop: LoopContext.get() }),
  },
  setup({ configProvider, db, log, session }) {
    const loop = createLoopManager({
      db,
      log,
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

export type LoopDb = DbContext<typeof loopDb>
