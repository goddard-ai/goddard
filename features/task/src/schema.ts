import type { IpcErrorRegistry, IpcErrorRegistryError } from "@goddard-ai/ipc"
import { SessionId } from "@goddard-ai/session/schema"
import { z } from "zod"

/** Client-visible daemon task error codes shared across daemon and SDK layers. */
export const TaskErrorCodes = {
  AlreadyClaimed: "task.already_claimed",
  EmptyUpdate: "task.empty_update",
  InvalidRoot: "task.invalid_root",
  InvalidToken: "task.invalid_token",
  LinkNotFound: "task.link_not_found",
  NotFound: "task.not_found",
} as const

export type TaskErrorCode = (typeof TaskErrorCodes)[keyof typeof TaskErrorCodes]

export const TaskErrorCode = z.enum([
  TaskErrorCodes.AlreadyClaimed,
  TaskErrorCodes.EmptyUpdate,
  TaskErrorCodes.InvalidRoot,
  TaskErrorCodes.InvalidToken,
  TaskErrorCodes.LinkNotFound,
  TaskErrorCodes.NotFound,
])

/** Tagged task id emitted by the daemon task store. */
export const TaskId = z.custom<`tsk_${string}`>(
  (value): value is `tsk_${string}` => typeof value === "string" && value.startsWith("tsk_"),
)

export type TaskId = z.infer<typeof TaskId>

/** Tagged task activity id emitted by the daemon task store. */
export const TaskActivityId = z.custom<`tac_${string}`>(
  (value): value is `tac_${string}` => typeof value === "string" && value.startsWith("tac_"),
)

export type TaskActivityId = z.infer<typeof TaskActivityId>

/** Tagged task link id emitted by the daemon task store. */
export const TaskLinkId = z.custom<`tln_${string}`>(
  (value): value is `tln_${string}` => typeof value === "string" && value.startsWith("tln_"),
)

export type TaskLinkId = z.infer<typeof TaskLinkId>

/** Compact lifecycle shared by task clients and agents. */
export const TaskStatus = z.enum(["todo", "active", "blocked", "done", "cancelled"])

export type TaskStatus = z.infer<typeof TaskStatus>

export const TaskPriority = z.number().int()

export type TaskPriority = z.infer<typeof TaskPriority>

export const TaskEditableField = z.enum(["title", "body", "priority"])

export type TaskEditableField = z.infer<typeof TaskEditableField>

/** Current canonical state for one repository task. */
export const Task = z.strictObject({
  id: TaskId,
  rootDir: z.string().min(1),
  title: z.string().trim().min(1),
  body: z.string().nullable(),
  status: TaskStatus,
  priority: TaskPriority,
  blockedReason: z.string().nullable(),
  claimedBySessionId: SessionId.nullable(),
  createdAt: z.number().int(),
  updatedAt: z.number().int(),
})

export type Task = z.infer<typeof Task>

/** Supported generic relation targets without duplicating linked feature state. */
export const TaskLinkKind = z.enum(["session", "workforce_request", "pull_request", "file", "url"])

export type TaskLinkKind = z.infer<typeof TaskLinkKind>

/** One task-owned reference to a Goddard entity, repository resource, or URL. */
export const TaskLink = z.strictObject({
  id: TaskLinkId,
  taskId: TaskId,
  rootDir: z.string().min(1),
  kind: TaskLinkKind,
  target: z.string().min(1),
  label: z.string().nullable(),
  createdAt: z.number().int(),
})

export type TaskLink = z.infer<typeof TaskLink>

/** Typed fact recorded for one accepted task mutation. */
export const TaskActivityPayload = z.discriminatedUnion("type", [
  z.strictObject({ type: z.literal("created") }),
  z.strictObject({
    type: z.literal("details_updated"),
    fields: z.array(TaskEditableField).min(1),
  }),
  z.strictObject({
    type: z.literal("status_changed"),
    from: TaskStatus,
    to: TaskStatus,
    blockedReason: z.string().nullable(),
  }),
  z.strictObject({ type: z.literal("claimed"), sessionId: SessionId }),
  z.strictObject({ type: z.literal("released"), sessionId: SessionId }),
  z.strictObject({ type: z.literal("note_added"), body: z.string().trim().min(1) }),
  z.strictObject({ type: z.literal("link_added"), linkId: TaskLinkId }),
  z.strictObject({ type: z.literal("link_removed"), linkId: TaskLinkId }),
])

export type TaskActivityPayload = z.infer<typeof TaskActivityPayload>

/** Immutable audit entry for one accepted task mutation. */
export const TaskActivity = z.strictObject({
  id: TaskActivityId,
  taskId: TaskId,
  rootDir: z.string().min(1),
  actorSessionId: SessionId.nullable(),
  payload: TaskActivityPayload,
  createdAt: z.number().int(),
})

export type TaskActivity = z.infer<typeof TaskActivity>

/** Event payload emitted after one task mutation commits. */
export const TaskChangedEvent = z.strictObject({
  task: Task,
  activity: TaskActivity,
})

export type TaskChangedEvent = z.infer<typeof TaskChangedEvent>

/** Structured client-visible daemon task errors keyed by exported identifiers. */
export const TaskIpcErrors = {
  AlreadyClaimed: {
    code: TaskErrorCodes.AlreadyClaimed,
    details: z.strictObject({
      taskId: TaskId,
      claimedBySessionId: SessionId,
    }),
  },
  EmptyUpdate: {
    code: TaskErrorCodes.EmptyUpdate,
    details: z.strictObject({ taskId: TaskId }),
  },
  InvalidRoot: {
    code: TaskErrorCodes.InvalidRoot,
    details: z.strictObject({ rootDir: z.string() }),
  },
  InvalidToken: {
    code: TaskErrorCodes.InvalidToken,
    details: z.undefined(),
  },
  LinkNotFound: {
    code: TaskErrorCodes.LinkNotFound,
    details: z.strictObject({
      taskId: TaskId,
      linkId: TaskLinkId,
    }),
  },
  NotFound: {
    code: TaskErrorCodes.NotFound,
    details: z.strictObject({ taskId: TaskId }),
  },
} as const satisfies IpcErrorRegistry

export type TaskIpcError = IpcErrorRegistryError<typeof TaskIpcErrors>

const RepositoryTaskRequest = {
  rootDir: z.string().min(1),
}

const TaskMutationActor = {
  token: z.string().optional(),
}

export const CreateTaskRequest = z.strictObject({
  ...RepositoryTaskRequest,
  ...TaskMutationActor,
  title: z.string().trim().min(1),
  body: z.string().nullable().optional(),
  priority: TaskPriority.optional(),
})

export type CreateTaskRequest = z.infer<typeof CreateTaskRequest>

export const GetTaskRequest = z.strictObject({
  ...RepositoryTaskRequest,
  id: TaskId,
})

export type GetTaskRequest = z.infer<typeof GetTaskRequest>

export const ListTasksRequest = z.strictObject({
  ...RepositoryTaskRequest,
  statuses: z.array(TaskStatus).optional(),
})

export type ListTasksRequest = z.infer<typeof ListTasksRequest>

export const UpdateTaskRequest = z.strictObject({
  ...RepositoryTaskRequest,
  ...TaskMutationActor,
  id: TaskId,
  title: z.string().trim().min(1).optional(),
  body: z.string().nullable().optional(),
  priority: TaskPriority.optional(),
})

export type UpdateTaskRequest = z.infer<typeof UpdateTaskRequest>

export const SetTaskStatusRequest = z.strictObject({
  ...RepositoryTaskRequest,
  ...TaskMutationActor,
  id: TaskId,
  status: TaskStatus,
  blockedReason: z.string().nullable().optional(),
})

export type SetTaskStatusRequest = z.infer<typeof SetTaskStatusRequest>

export const ClaimTaskRequest = z.strictObject({
  ...RepositoryTaskRequest,
  ...TaskMutationActor,
  id: TaskId,
  sessionId: SessionId,
})

export type ClaimTaskRequest = z.infer<typeof ClaimTaskRequest>

export const ReleaseTaskRequest = z.strictObject({
  ...RepositoryTaskRequest,
  ...TaskMutationActor,
  id: TaskId,
})

export type ReleaseTaskRequest = z.infer<typeof ReleaseTaskRequest>

export const AddTaskNoteRequest = z.strictObject({
  ...RepositoryTaskRequest,
  ...TaskMutationActor,
  id: TaskId,
  body: z.string().trim().min(1),
})

export type AddTaskNoteRequest = z.infer<typeof AddTaskNoteRequest>

export const AddTaskLinkRequest = z.strictObject({
  ...RepositoryTaskRequest,
  ...TaskMutationActor,
  id: TaskId,
  kind: TaskLinkKind,
  target: z.string().min(1),
  label: z.string().nullable().optional(),
})

export type AddTaskLinkRequest = z.infer<typeof AddTaskLinkRequest>

export const RemoveTaskLinkRequest = z.strictObject({
  ...RepositoryTaskRequest,
  ...TaskMutationActor,
  id: TaskId,
  linkId: TaskLinkId,
})

export type RemoveTaskLinkRequest = z.infer<typeof RemoveTaskLinkRequest>

export type TaskMutationResponse = {
  task: Task
  activity: TaskActivity | null
}

export type AddTaskLinkResponse = TaskMutationResponse & {
  link: TaskLink
}

export type GetTaskResponse = {
  task: Task
  links: TaskLink[]
  activity: TaskActivity[]
}

export type ListTasksResponse = {
  tasks: Task[]
}
