import type { EventBus } from "@goddard-ai/daemon-plugin"
import type { SessionId } from "@goddard-ai/session/schema"
import type { KindInput } from "kindstore"

import type { TaskStore } from "../daemon.ts"
import type { taskEvents } from "../events.ts"
import {
  TaskErrorCodes,
  type AddTaskLinkRequest,
  type AddTaskNoteRequest,
  type ClaimTaskRequest,
  type CreateTaskRequest,
  type GetTaskRequest,
  type ListTasksRequest,
  type ReleaseTaskRequest,
  type RemoveTaskLinkRequest,
  type SetTaskStatusRequest,
  type Task,
  type TaskActivity,
  type TaskActivityPayload,
  type TaskEditableField,
  type TaskId,
  type UpdateTaskRequest,
} from "../schema.ts"
import { createTaskIpcError } from "./ipc-error.ts"
import { normalizeTaskRootDir } from "./paths.ts"

const DEFAULT_TASK_PRIORITY = 0

type TaskInput = KindInput<TaskStore["schema"]["tasks"]>
type TaskActivityInput = KindInput<TaskStore["schema"]["taskActivities"]>
type TaskLinkInput = KindInput<TaskStore["schema"]["taskLinks"]>
type TaskEventEmitter = Pick<EventBus<typeof taskEvents>, "emit">
type MutationActor = { actorSessionId: SessionId | null }
type ManagerMutationInput<TInput extends { token?: string }> = Omit<TInput, "token"> & MutationActor

type TaskManagerOptions = {
  db: TaskStore
  events: TaskEventEmitter
  now?: () => number
  normalizeRootDir?: (rootDir: string) => Promise<string>
}

function resolveNewTask(input: CreateTaskRequest, rootDir: string, timestamp: number): TaskInput {
  return {
    rootDir,
    title: input.title,
    body: input.body ?? null,
    status: "todo",
    priority: input.priority ?? DEFAULT_TASK_PRIORITY,
    blockedReason: null,
    claimedBySessionId: null,
    createdAt: timestamp,
    updatedAt: timestamp,
  }
}

/** Creates the daemon-owned task manager that centralizes task state and activity writes. */
export function createTaskManager({
  db,
  events,
  now = Date.now,
  normalizeRootDir = normalizeTaskRootDir,
}: TaskManagerOptions) {
  function requireTask(taskId: TaskId, rootDir: string) {
    const task = db.tasks.get(taskId) ?? null
    if (!task || task.rootDir !== rootDir) {
      throw createTaskIpcError(TaskErrorCodes.NotFound, { taskId })
    }
    return task
  }

  function appendActivity(
    task: Task,
    actorSessionId: SessionId | null,
    payload: TaskActivityPayload,
    timestamp: number,
  ) {
    return db.taskActivities.create({
      taskId: task.id,
      rootDir: task.rootDir,
      actorSessionId,
      payload,
      createdAt: timestamp,
    } satisfies TaskActivityInput)
  }

  function emitChange(task: Task, activity: TaskActivity) {
    void events.emit("task.changed", { task, activity })
  }

  async function createTask(input: ManagerMutationInput<CreateTaskRequest>) {
    const rootDir = await normalizeRootDir(input.rootDir)
    const timestamp = now()
    let task!: Task
    let activity!: TaskActivity

    db.batch(() => {
      task = db.tasks.create(resolveNewTask(input, rootDir, timestamp))
      activity = appendActivity(task, input.actorSessionId, { type: "created" }, timestamp)
    })

    emitChange(task, activity)
    return { task, activity }
  }

  async function getTask(input: GetTaskRequest) {
    const rootDir = await normalizeRootDir(input.rootDir)
    const task = requireTask(input.id, rootDir)
    const links = db.taskLinks.findMany({
      where: { taskId: task.id },
      orderBy: { createdAt: "asc", id: "asc" },
    })
    const activity = db.taskActivities.findMany({
      where: { taskId: task.id },
      orderBy: { createdAt: "asc", id: "asc" },
    })

    return { task, links, activity }
  }

  async function listTasks(input: ListTasksRequest) {
    const rootDir = await normalizeRootDir(input.rootDir)
    if (input.statuses?.length === 0) {
      return { tasks: [] }
    }

    return {
      tasks: db.tasks.findMany({
        where: {
          rootDir,
          ...(input.statuses && { status: { in: input.statuses } }),
        },
        orderBy: { priority: "desc", updatedAt: "desc", id: "desc" },
      }),
    }
  }

  async function updateTask(input: ManagerMutationInput<UpdateTaskRequest>) {
    const rootDir = await normalizeRootDir(input.rootDir)
    const existing = requireTask(input.id, rootDir)
    const fields: TaskEditableField[] = []
    const patch: Partial<TaskInput> = {}

    if (input.title !== undefined && input.title !== existing.title) {
      fields.push("title")
      patch.title = input.title
    }
    if (input.body !== undefined && input.body !== existing.body) {
      fields.push("body")
      patch.body = input.body
    }
    if (input.priority !== undefined && input.priority !== existing.priority) {
      fields.push("priority")
      patch.priority = input.priority
    }
    if (input.title === undefined && input.body === undefined && input.priority === undefined) {
      throw createTaskIpcError(TaskErrorCodes.EmptyUpdate, { taskId: existing.id })
    }
    if (fields.length === 0) {
      return { task: existing, activity: null }
    }

    const timestamp = now()
    let task!: Task
    let activity!: TaskActivity
    db.batch(() => {
      task = db.tasks.update(existing.id, { ...patch, updatedAt: timestamp })!
      activity = appendActivity(
        task,
        input.actorSessionId,
        { type: "details_updated", fields },
        timestamp,
      )
    })

    emitChange(task, activity)
    return { task, activity }
  }

  async function setTaskStatus(input: ManagerMutationInput<SetTaskStatusRequest>) {
    const rootDir = await normalizeRootDir(input.rootDir)
    const existing = requireTask(input.id, rootDir)
    const blockedReason =
      input.status === "blocked"
        ? (input.blockedReason ?? (existing.status === "blocked" ? existing.blockedReason : null))
        : null
    if (input.status === existing.status && blockedReason === existing.blockedReason) {
      return { task: existing, activity: null }
    }

    const timestamp = now()
    let task!: Task
    let activity!: TaskActivity
    db.batch(() => {
      task = db.tasks.update(existing.id, {
        status: input.status,
        blockedReason,
        updatedAt: timestamp,
      })!
      activity = appendActivity(
        task,
        input.actorSessionId,
        {
          type: "status_changed",
          from: existing.status,
          to: input.status,
          blockedReason,
        },
        timestamp,
      )
    })

    emitChange(task, activity)
    return { task, activity }
  }

  async function claimTask(input: ManagerMutationInput<ClaimTaskRequest>) {
    const rootDir = await normalizeRootDir(input.rootDir)
    const existing = requireTask(input.id, rootDir)
    if (existing.claimedBySessionId === input.sessionId) {
      return { task: existing, activity: null }
    }
    if (existing.claimedBySessionId) {
      throw createTaskIpcError(TaskErrorCodes.AlreadyClaimed, {
        taskId: existing.id,
        claimedBySessionId: existing.claimedBySessionId,
      })
    }

    const timestamp = now()
    let task!: Task
    let activity!: TaskActivity
    db.batch(() => {
      task = db.tasks.update(existing.id, {
        claimedBySessionId: input.sessionId,
        updatedAt: timestamp,
      })!
      activity = appendActivity(
        task,
        input.actorSessionId,
        { type: "claimed", sessionId: input.sessionId },
        timestamp,
      )
    })

    emitChange(task, activity)
    return { task, activity }
  }

  async function releaseTask(input: ManagerMutationInput<ReleaseTaskRequest>) {
    const rootDir = await normalizeRootDir(input.rootDir)
    const existing = requireTask(input.id, rootDir)
    if (!existing.claimedBySessionId) {
      return { task: existing, activity: null }
    }

    const releasedSessionId = existing.claimedBySessionId
    const timestamp = now()
    let task!: Task
    let activity!: TaskActivity
    db.batch(() => {
      task = db.tasks.update(existing.id, {
        claimedBySessionId: null,
        updatedAt: timestamp,
      })!
      activity = appendActivity(
        task,
        input.actorSessionId,
        { type: "released", sessionId: releasedSessionId },
        timestamp,
      )
    })

    emitChange(task, activity)
    return { task, activity }
  }

  async function addTaskNote(input: ManagerMutationInput<AddTaskNoteRequest>) {
    const rootDir = await normalizeRootDir(input.rootDir)
    const existing = requireTask(input.id, rootDir)
    const timestamp = now()
    let task!: Task
    let activity!: TaskActivity
    db.batch(() => {
      task = db.tasks.update(existing.id, { updatedAt: timestamp })!
      activity = appendActivity(
        task,
        input.actorSessionId,
        { type: "note_added", body: input.body },
        timestamp,
      )
    })

    emitChange(task, activity)
    return { task, activity }
  }

  async function addTaskLink(input: ManagerMutationInput<AddTaskLinkRequest>) {
    const rootDir = await normalizeRootDir(input.rootDir)
    const existing = requireTask(input.id, rootDir)
    const timestamp = now()
    let task!: Task
    let link!: ReturnType<typeof db.taskLinks.create>
    let activity!: TaskActivity
    db.batch(() => {
      link = db.taskLinks.create({
        taskId: existing.id,
        rootDir,
        kind: input.kind,
        target: input.target,
        label: input.label ?? null,
        createdAt: timestamp,
      } satisfies TaskLinkInput)
      task = db.tasks.update(existing.id, { updatedAt: timestamp })!
      activity = appendActivity(
        task,
        input.actorSessionId,
        { type: "link_added", linkId: link.id },
        timestamp,
      )
    })

    emitChange(task, activity)
    return { task, link, activity }
  }

  async function removeTaskLink(input: ManagerMutationInput<RemoveTaskLinkRequest>) {
    const rootDir = await normalizeRootDir(input.rootDir)
    const existing = requireTask(input.id, rootDir)
    const link = db.taskLinks.get(input.linkId) ?? null
    if (!link || link.taskId !== existing.id) {
      throw createTaskIpcError(TaskErrorCodes.LinkNotFound, {
        taskId: existing.id,
        linkId: input.linkId,
      })
    }

    const timestamp = now()
    let task!: Task
    let activity!: TaskActivity
    db.batch(() => {
      db.taskLinks.delete(link.id)
      task = db.tasks.update(existing.id, { updatedAt: timestamp })!
      activity = appendActivity(
        task,
        input.actorSessionId,
        { type: "link_removed", linkId: link.id },
        timestamp,
      )
    })

    emitChange(task, activity)
    return { task, activity }
  }

  return {
    createTask,
    getTask,
    listTasks,
    updateTask,
    setTaskStatus,
    claimTask,
    releaseTask,
    addTaskNote,
    addTaskLink,
    removeTaskLink,
  }
}

export type TaskManager = ReturnType<typeof createTaskManager>
