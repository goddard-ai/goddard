import { randomUUID } from "node:crypto"
import { mkdtemp, realpath, rm } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { IpcClientError } from "@goddard-ai/ipc"
import type { SessionId } from "@goddard-ai/session/schema"
import { afterEach, beforeEach, expect, test } from "bun:test"
import { kindstore, type Kindstore } from "kindstore"

import { createTaskManager, taskPlugin } from "../src/daemon.ts"
import { TaskErrorCodes, type TaskChangedEvent } from "../src/schema.ts"

let store: Kindstore<(typeof taskPlugin)["db"]["schema"], {}>
let tempRoot: string
let repositoryRoot: string
let secondRepositoryRoot: string
let timestamp: number
let events: TaskChangedEvent[]

beforeEach(async () => {
  tempRoot = await mkdtemp(join(tmpdir(), "goddard-task-test-"))
  repositoryRoot = await realpath(tempRoot)
  secondRepositoryRoot = await realpath(await mkdtemp(join(tempRoot, "second-")))
  store = kindstore({
    filename: ":memory:",
    schema: taskPlugin.db.schema,
  })
  timestamp = 1_000
  events = []
})

afterEach(async () => {
  store.close()
  await rm(tempRoot, { recursive: true, force: true })
})

function newSessionId() {
  return `ses_${randomUUID()}` as SessionId
}

function createTestTaskManager() {
  return createTaskManager({
    db: store,
    now: () => timestamp++,
    events: {
      emit: async (_name, event) => {
        events.push(event)
      },
    },
  })
}

test("task creation stores canonical defaults and matching activity atomically", async () => {
  const tasks = createTestTaskManager()
  const result = await tasks.createTask({
    rootDir: join(repositoryRoot, "."),
    title: "Ship task support",
    actorSessionId: null,
  })

  expect(result.task).toMatchObject({
    rootDir: repositoryRoot,
    title: "Ship task support",
    body: null,
    status: "todo",
    priority: 0,
    blockedReason: null,
    claimedBySessionId: null,
  })
  expect(result.activity).toMatchObject({
    taskId: result.task.id,
    rootDir: repositoryRoot,
    actorSessionId: null,
    payload: { type: "created" },
  })
  expect(store.tasks.get(result.task.id)).toEqual(result.task)
  expect(store.taskActivities.findMany({ where: { taskId: result.task.id } })).toEqual([
    result.activity,
  ])
  expect(events).toEqual([{ task: result.task, activity: result.activity }])
})

test("task mutations preserve typed immutable activity and current details", async () => {
  const tasks = createTestTaskManager()
  const actorSessionId = newSessionId()
  const created = await tasks.createTask({
    rootDir: repositoryRoot,
    title: "Initial title",
    actorSessionId,
  })
  await tasks.updateTask({
    rootDir: repositoryRoot,
    id: created.task.id,
    title: "Refined title",
    priority: 2,
    actorSessionId,
  })
  await tasks.setTaskStatus({
    rootDir: repositoryRoot,
    id: created.task.id,
    status: "blocked",
    blockedReason: "Waiting on API decision",
    actorSessionId,
  })
  await tasks.addTaskNote({
    rootDir: repositoryRoot,
    id: created.task.id,
    body: "The SDK contract is ready.",
    actorSessionId,
  })
  const addedLink = await tasks.addTaskLink({
    rootDir: repositoryRoot,
    id: created.task.id,
    kind: "url",
    target: "https://example.com/design",
    label: "Design",
    actorSessionId,
  })
  await tasks.removeTaskLink({
    rootDir: repositoryRoot,
    id: created.task.id,
    linkId: addedLink.link.id,
    actorSessionId,
  })

  const details = await tasks.getTask({ rootDir: repositoryRoot, id: created.task.id })
  expect(details.task).toMatchObject({
    title: "Refined title",
    priority: 2,
    status: "blocked",
    blockedReason: "Waiting on API decision",
  })
  expect(details.links).toEqual([])
  expect(details.activity.map((entry) => entry.payload)).toEqual([
    { type: "created" },
    { type: "details_updated", fields: ["title", "priority"] },
    {
      type: "status_changed",
      from: "todo",
      to: "blocked",
      blockedReason: "Waiting on API decision",
    },
    { type: "note_added", body: "The SDK contract is ready." },
    { type: "link_added", linkId: addedLink.link.id },
    { type: "link_removed", linkId: addedLink.link.id },
  ])
})

test("claims are idempotent for the owner and reject competing sessions without activity", async () => {
  const tasks = createTestTaskManager()
  const firstSessionId = newSessionId()
  const secondSessionId = newSessionId()
  const created = await tasks.createTask({
    rootDir: repositoryRoot,
    title: "Claimable task",
    actorSessionId: null,
  })
  const claimed = await tasks.claimTask({
    rootDir: repositoryRoot,
    id: created.task.id,
    sessionId: firstSessionId,
    actorSessionId: firstSessionId,
  })
  const repeated = await tasks.claimTask({
    rootDir: repositoryRoot,
    id: created.task.id,
    sessionId: firstSessionId,
    actorSessionId: firstSessionId,
  })

  expect(claimed.task.claimedBySessionId).toBe(firstSessionId)
  expect(repeated.activity).toBeNull()
  expect(store.taskActivities.findMany({ where: { taskId: created.task.id } })).toHaveLength(2)

  try {
    await tasks.claimTask({
      rootDir: repositoryRoot,
      id: created.task.id,
      sessionId: secondSessionId,
      actorSessionId: secondSessionId,
    })
    throw new Error("Expected a competing claim to fail")
  } catch (error) {
    expect(error).toBeInstanceOf(IpcClientError)
    expect(error).toHaveProperty("code", TaskErrorCodes.AlreadyClaimed)
    expect(error).toHaveProperty("details", {
      taskId: created.task.id,
      claimedBySessionId: firstSessionId,
    })
  }

  expect(store.tasks.get(created.task.id)?.claimedBySessionId).toBe(firstSessionId)
  expect(store.taskActivities.findMany({ where: { taskId: created.task.id } })).toHaveLength(2)
})

test("repository task lists use priority, freshness, and id for deterministic ordering", async () => {
  const tasks = createTestTaskManager()
  const low = await tasks.createTask({
    rootDir: repositoryRoot,
    title: "Low priority",
    priority: 0,
    actorSessionId: null,
  })
  const olderHigh = await tasks.createTask({
    rootDir: repositoryRoot,
    title: "Older high priority",
    priority: 2,
    actorSessionId: null,
  })
  const newerHigh = await tasks.createTask({
    rootDir: repositoryRoot,
    title: "Newer high priority",
    priority: 2,
    actorSessionId: null,
  })
  await tasks.createTask({
    rootDir: secondRepositoryRoot,
    title: "Other repository",
    priority: 99,
    actorSessionId: null,
  })
  await tasks.setTaskStatus({
    rootDir: repositoryRoot,
    id: low.task.id,
    status: "done",
    actorSessionId: null,
  })

  const all = await tasks.listTasks({ rootDir: repositoryRoot })
  expect(all.tasks.map((task) => task.id)).toEqual([
    newerHigh.task.id,
    olderHigh.task.id,
    low.task.id,
  ])
  const done = await tasks.listTasks({ rootDir: repositoryRoot, statuses: ["done"] })
  expect(done.tasks.map((task) => task.id)).toEqual([low.task.id])
  expect(await tasks.listTasks({ rootDir: repositoryRoot, statuses: [] })).toEqual({ tasks: [] })
})

test("rejected updates and missing links leave state and activity unchanged", async () => {
  const tasks = createTestTaskManager()
  const created = await tasks.createTask({
    rootDir: repositoryRoot,
    title: "Protected history",
    actorSessionId: null,
  })

  await expect(
    tasks.updateTask({
      rootDir: repositoryRoot,
      id: created.task.id,
      actorSessionId: null,
    }),
  ).rejects.toHaveProperty("code", TaskErrorCodes.EmptyUpdate)
  await expect(
    tasks.removeTaskLink({
      rootDir: repositoryRoot,
      id: created.task.id,
      linkId: "tln_missing",
      actorSessionId: null,
    }),
  ).rejects.toHaveProperty("code", TaskErrorCodes.LinkNotFound)

  expect(store.tasks.get(created.task.id)).toEqual(created.task)
  expect(store.taskActivities.findMany({ where: { taskId: created.task.id } })).toEqual([
    created.activity,
  ])
})
