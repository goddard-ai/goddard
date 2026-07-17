import { definePlugin, type DbContext } from "@goddard-ai/daemon-plugin"
import { sessionPlugin } from "@goddard-ai/session/daemon"
import { kind } from "kindstore"

import { taskIpcRoutes } from "./daemon-ipc.ts"
import { createTaskIpcError } from "./daemon/ipc-error.ts"
import { createTaskManager } from "./daemon/manager.ts"
import { taskEvents } from "./events.ts"
import { Task, TaskActivity, TaskErrorCodes, TaskLink } from "./schema.ts"

export { createTaskManager, type TaskManager } from "./daemon/manager.ts"

const taskDb = {
  tasks: kind("tsk", Task.omit({ id: true }))
    .index("rootDir", { type: "text" })
    .index("status")
    .index("claimedBySessionId", { type: "text" })
    .multi("rootDir_priority_updatedAt_id", {
      rootDir: "asc",
      priority: "desc",
      updatedAt: "desc",
      id: "desc",
    }),
  taskActivities: kind("tac", TaskActivity.omit({ id: true }))
    .index("taskId", { type: "text" })
    .multi("taskId_createdAt_id", {
      taskId: "asc",
      createdAt: "asc",
      id: "asc",
    }),
  taskLinks: kind("tln", TaskLink.omit({ id: true }))
    .index("taskId", { type: "text" })
    .multi("taskId_createdAt_id", {
      taskId: "asc",
      createdAt: "asc",
      id: "asc",
    }),
}

export const taskPlugin = definePlugin({
  name: "task",
  consumes: [sessionPlugin],
  db: { schema: taskDb },
  events: taskEvents,
  ipcRoutes: taskIpcRoutes,
  setup({ db, events, ipc, session }) {
    const task = createTaskManager({ db, events })

    async function withActor<TInput extends { token?: string }>(input: TInput) {
      const { token, ...request } = input
      if (!token) {
        return { ...request, actorSessionId: null }
      }

      const actor = await session.resolveTokenScope(token)
      if (!actor) {
        throw createTaskIpcError(TaskErrorCodes.InvalidToken)
      }
      ipc.requestContext.setSessionId(actor.sessionId)
      return { ...request, actorSessionId: actor.sessionId }
    }

    return {
      ipcHandlers: {
        task: {
          create: async ({ body }) => task.createTask(await withActor(body)),
          get: async ({ body }) => task.getTask(body),
          list: async ({ body }) => task.listTasks(body),
          update: async ({ body }) => task.updateTask(await withActor(body)),
          setStatus: async ({ body }) => task.setTaskStatus(await withActor(body)),
          claim: async ({ body }) => {
            await session.getSession(body.sessionId)
            return task.claimTask(await withActor(body))
          },
          release: async ({ body }) => task.releaseTask(await withActor(body)),
          addNote: async ({ body }) => task.addTaskNote(await withActor(body)),
          addLink: async ({ body }) => task.addTaskLink(await withActor(body)),
          removeLink: async ({ body }) => task.removeTaskLink(await withActor(body)),
        },
      },
    }
  },
})

export type TaskStore = DbContext<typeof taskDb>
