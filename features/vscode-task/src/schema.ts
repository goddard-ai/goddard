import type { IpcErrorRegistry, IpcErrorRegistryError } from "@goddard-ai/ipc"
import { z } from "zod"

/** Client-visible daemon workspace-task error codes. */
export const VscodeTaskErrorCodes = {
  ConnectionNotFound: "vscode_task.connection_not_found",
  ConnectionStreamActive: "vscode_task.connection_stream_active",
  DocumentInvalid: "vscode_task.document_invalid",
  DocumentNotFound: "vscode_task.document_not_found",
  DocumentUnreadable: "vscode_task.document_unreadable",
  RunFailed: "vscode_task.run_failed",
  RunNotFound: "vscode_task.run_not_found",
  TaskUnavailable: "vscode_task.task_unavailable",
} as const

export type VscodeTaskErrorCode = (typeof VscodeTaskErrorCodes)[keyof typeof VscodeTaskErrorCodes]

export const VscodeTaskErrorCode = z.enum([
  VscodeTaskErrorCodes.ConnectionNotFound,
  VscodeTaskErrorCodes.ConnectionStreamActive,
  VscodeTaskErrorCodes.DocumentInvalid,
  VscodeTaskErrorCodes.DocumentNotFound,
  VscodeTaskErrorCodes.DocumentUnreadable,
  VscodeTaskErrorCodes.RunFailed,
  VscodeTaskErrorCodes.RunNotFound,
  VscodeTaskErrorCodes.TaskUnavailable,
])

export const VscodeTaskConnectionId = z.custom<`vstc_${string}`>(
  (value): value is `vstc_${string}` => typeof value === "string" && value.startsWith("vstc_"),
)

export type VscodeTaskConnectionId = z.infer<typeof VscodeTaskConnectionId>

export const VscodeTaskRunId = z.custom<`vstr_${string}`>(
  (value): value is `vstr_${string}` => typeof value === "string" && value.startsWith("vstr_"),
)

export type VscodeTaskRunId = z.infer<typeof VscodeTaskRunId>

export const VscodeTaskKind = z.enum(["shell", "process", "compound"])

export type VscodeTaskKind = z.infer<typeof VscodeTaskKind>

export const VscodeTaskDependsOrder = z.enum(["parallel", "sequence"])

export type VscodeTaskDependsOrder = z.infer<typeof VscodeTaskDependsOrder>

export const VscodeTaskGroup = z.strictObject({
  kind: z.enum(["build", "test"]),
  isDefault: z.boolean(),
})

export type VscodeTaskGroup = z.infer<typeof VscodeTaskGroup>

/** One analyzed task, including unsupported records that callers should keep visible. */
export const VscodeTaskSummary = z.strictObject({
  id: z.string().min(1),
  label: z.string().min(1).nullable(),
  supported: z.boolean(),
  kind: VscodeTaskKind.nullable(),
  hidden: z.boolean(),
  group: VscodeTaskGroup.nullable(),
  dependsOn: z.array(z.string()),
  dependsOrder: VscodeTaskDependsOrder,
  issues: z.array(z.string()),
  unsupportedReason: z.string().nullable(),
})

export type VscodeTaskSummary = z.infer<typeof VscodeTaskSummary>

export const VscodeTaskAnalysisErrorCode = z.enum([
  "invalid_root",
  "invalid_tasks",
  "unsupported_version",
  "duplicate_label",
  "unknown_dependency",
  "dependency_cycle",
  "invalid_task",
])

export const VscodeTaskAnalysisError = z.strictObject({
  code: VscodeTaskAnalysisErrorCode,
  message: z.string(),
  taskId: z.string().nullable(),
  label: z.string().nullable(),
})

export type VscodeTaskAnalysisError = z.infer<typeof VscodeTaskAnalysisError>

export const InspectVscodeTasksRequest = z.strictObject({
  workspaceRoot: z.string().min(1),
})

export type InspectVscodeTasksRequest = z.infer<typeof InspectVscodeTasksRequest>

export const InspectVscodeTasksResponse = z.strictObject({
  workspaceRoot: z.string().min(1),
  sourcePath: z.string().min(1),
  version: z.string().nullable(),
  tasks: z.array(VscodeTaskSummary),
  errors: z.array(VscodeTaskAnalysisError),
})

export type InspectVscodeTasksResponse = z.infer<typeof InspectVscodeTasksResponse>

/** Fully resolved task and dependency plan exposed for explicit pre-run inspection. */
export const ResolvedVscodeTaskPlan = z.strictObject({
  label: z.string().min(1),
  kind: VscodeTaskKind,
  command: z.string().nullable(),
  args: z.array(z.string()),
  cwd: z.string().min(1),
  env: z.record(z.string(), z.string()),
  shell: z
    .strictObject({
      executable: z.string().nullable(),
      args: z.array(z.string()).nullable(),
    })
    .nullable(),
  hidden: z.boolean(),
  group: VscodeTaskGroup.nullable(),
  dependsOn: z.array(z.string()),
  dependsOrder: VscodeTaskDependsOrder,
  get dependencies() {
    return z.array(ResolvedVscodeTaskPlan)
  },
})

export type ResolvedVscodeTaskPlan = z.infer<typeof ResolvedVscodeTaskPlan>

export const ResolveVscodeTaskRequest = InspectVscodeTasksRequest.extend({
  label: z.string().min(1),
})

export type ResolveVscodeTaskRequest = z.infer<typeof ResolveVscodeTaskRequest>

export const ResolveVscodeTaskResponse = z.strictObject({
  task: ResolvedVscodeTaskPlan,
})

export type ResolveVscodeTaskResponse = z.infer<typeof ResolveVscodeTaskResponse>

export const VscodeTaskConnectRequest = z.strictObject({})

export type VscodeTaskConnectRequest = z.infer<typeof VscodeTaskConnectRequest>

export const VscodeTaskConnectResponse = z.strictObject({
  connectionId: VscodeTaskConnectionId,
})

export type VscodeTaskConnectResponse = z.infer<typeof VscodeTaskConnectResponse>

export const VscodeTaskConnectionParams = z.strictObject({
  connectionId: VscodeTaskConnectionId,
})

export type VscodeTaskConnectionParams = z.infer<typeof VscodeTaskConnectionParams>

export const VscodeTaskRunRequest = VscodeTaskConnectionParams.extend({
  workspaceRoot: z.string().min(1),
  label: z.string().min(1),
})

export type VscodeTaskRunRequest = z.infer<typeof VscodeTaskRunRequest>

export const VscodeTaskRunResponse = z.strictObject({
  runId: VscodeTaskRunId,
  label: z.string().min(1),
})

export type VscodeTaskRunResponse = z.infer<typeof VscodeTaskRunResponse>

export const VscodeTaskCancelRequest = VscodeTaskConnectionParams.extend({
  runId: VscodeTaskRunId,
})

export type VscodeTaskCancelRequest = z.infer<typeof VscodeTaskCancelRequest>

export const VscodeTaskMutationResponse = z.strictObject({
  accepted: z.literal(true),
})

export type VscodeTaskMutationResponse = z.infer<typeof VscodeTaskMutationResponse>

export const VscodeTaskRunStatus = z.enum(["success", "failed", "canceled"])

export const VscodeTaskRunResult = z.strictObject({
  status: VscodeTaskRunStatus,
  label: z.string().min(1),
  failedTaskLabel: z.string().nullable(),
  exitCode: z.number().int().nullable(),
  signal: z.string().nullable(),
})

export type VscodeTaskRunResult = z.infer<typeof VscodeTaskRunResult>

const VscodeTaskRunEventBase = z.strictObject({
  connectionId: VscodeTaskConnectionId,
  runId: VscodeTaskRunId,
})

export const VscodeTaskRunStartedEvent = VscodeTaskRunEventBase.extend({
  type: z.literal("vscode-task.run.started"),
  label: z.string().min(1),
})

export const VscodeTaskProcessStartedEvent = VscodeTaskRunEventBase.extend({
  type: z.literal("vscode-task.process.started"),
  label: z.string().min(1),
  kind: z.enum(["shell", "process"]),
})

export const VscodeTaskProcessOutputEvent = VscodeTaskRunEventBase.extend({
  type: z.literal("vscode-task.process.output"),
  label: z.string().min(1),
  stream: z.enum(["stdout", "stderr"]),
  data: z.string().min(1),
})

export const VscodeTaskProcessExitedEvent = VscodeTaskRunEventBase.extend({
  type: z.literal("vscode-task.process.exited"),
  label: z.string().min(1),
  exitCode: z.number().int().nullable(),
  signal: z.string().nullable(),
})

export const VscodeTaskProcessCanceledEvent = VscodeTaskRunEventBase.extend({
  type: z.literal("vscode-task.process.canceled"),
  label: z.string().min(1),
  signal: z.string().nullable(),
})

export const VscodeTaskRunCompletedEvent = VscodeTaskRunEventBase.extend({
  type: z.literal("vscode-task.run.completed"),
  result: VscodeTaskRunResult,
})

export const VscodeTaskRunFailedEvent = VscodeTaskRunEventBase.extend({
  type: z.literal("vscode-task.run.failed"),
  code: z.literal(VscodeTaskErrorCodes.RunFailed),
  details: z.strictObject({
    label: z.string().min(1),
  }),
})

/** Connection-scoped workspace-task lifecycle and PTY output event. */
export const VscodeTaskDaemonEvent = z.discriminatedUnion("type", [
  VscodeTaskRunStartedEvent,
  VscodeTaskProcessStartedEvent,
  VscodeTaskProcessOutputEvent,
  VscodeTaskProcessExitedEvent,
  VscodeTaskProcessCanceledEvent,
  VscodeTaskRunCompletedEvent,
  VscodeTaskRunFailedEvent,
])

export type VscodeTaskDaemonEvent = z.infer<typeof VscodeTaskDaemonEvent>

/** Structured workspace-task IPC errors keyed by exported identifiers. */
export const VscodeTaskIpcErrors = {
  ConnectionNotFound: {
    code: VscodeTaskErrorCodes.ConnectionNotFound,
    details: z.strictObject({ connectionId: z.string() }),
  },
  ConnectionStreamActive: {
    code: VscodeTaskErrorCodes.ConnectionStreamActive,
    details: z.strictObject({ connectionId: VscodeTaskConnectionId }),
  },
  DocumentInvalid: {
    code: VscodeTaskErrorCodes.DocumentInvalid,
    details: z.strictObject({ sourcePath: z.string(), diagnostics: z.array(z.string()) }),
  },
  DocumentNotFound: {
    code: VscodeTaskErrorCodes.DocumentNotFound,
    details: z.strictObject({ sourcePath: z.string() }),
  },
  DocumentUnreadable: {
    code: VscodeTaskErrorCodes.DocumentUnreadable,
    details: z.strictObject({ sourcePath: z.string() }),
  },
  RunFailed: {
    code: VscodeTaskErrorCodes.RunFailed,
    details: z.strictObject({ runId: VscodeTaskRunId, label: z.string() }),
  },
  RunNotFound: {
    code: VscodeTaskErrorCodes.RunNotFound,
    details: z.strictObject({ connectionId: VscodeTaskConnectionId, runId: VscodeTaskRunId }),
  },
  TaskUnavailable: {
    code: VscodeTaskErrorCodes.TaskUnavailable,
    details: z.strictObject({ label: z.string() }),
  },
} as const satisfies IpcErrorRegistry

export type VscodeTaskIpcError = IpcErrorRegistryError<typeof VscodeTaskIpcErrors>
