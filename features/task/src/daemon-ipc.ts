import { $type, defineIpcRoutes, http, metadata } from "@goddard-ai/ipc"

import {
  AddTaskLinkRequest,
  AddTaskNoteRequest,
  ClaimTaskRequest,
  CreateTaskRequest,
  GetTaskRequest,
  ListTasksRequest,
  ReleaseTaskRequest,
  RemoveTaskLinkRequest,
  SetTaskStatusRequest,
  UpdateTaskRequest,
  type AddTaskLinkResponse,
  type GetTaskResponse,
  type ListTasksResponse,
  type TaskMutationResponse,
} from "./schema.ts"

export const taskIpcRoutes = defineIpcRoutes({
  task: http.resource("task", {
    ...metadata({ description: "Repository task management." }),
    create: http.post("create", {
      ...metadata({ description: "Creates one repository task." }),
      body: CreateTaskRequest,
      response: $type<TaskMutationResponse>(),
    }),
    get: http.post("get", {
      ...metadata({ description: "Gets one task with its links and activity." }),
      body: GetTaskRequest,
      response: $type<GetTaskResponse>(),
    }),
    list: http.post("list", {
      ...metadata({ description: "Lists repository tasks in deterministic order." }),
      body: ListTasksRequest,
      response: $type<ListTasksResponse>(),
    }),
    update: http.post("update", {
      ...metadata({ description: "Updates task content or priority." }),
      body: UpdateTaskRequest,
      response: $type<TaskMutationResponse>(),
    }),
    setStatus: http.post("set-status", {
      ...metadata({ description: "Changes one task's lifecycle status." }),
      body: SetTaskStatusRequest,
      response: $type<TaskMutationResponse>(),
    }),
    claim: http.post("claim", {
      ...metadata({ description: "Claims one task for a daemon-managed session." }),
      body: ClaimTaskRequest,
      response: $type<TaskMutationResponse>(),
    }),
    release: http.post("release", {
      ...metadata({ description: "Explicitly releases one task claim." }),
      body: ReleaseTaskRequest,
      response: $type<TaskMutationResponse>(),
    }),
    addNote: http.post("add-note", {
      ...metadata({ description: "Appends one immutable task note." }),
      body: AddTaskNoteRequest,
      response: $type<TaskMutationResponse>(),
    }),
    addLink: http.post("add-link", {
      ...metadata({ description: "Adds one generic task link." }),
      body: AddTaskLinkRequest,
      response: $type<AddTaskLinkResponse>(),
    }),
    removeLink: http.post("remove-link", {
      ...metadata({ description: "Removes one task link." }),
      body: RemoveTaskLinkRequest,
      response: $type<TaskMutationResponse>(),
    }),
  }),
})
