import { readFile } from "node:fs/promises"
import { resolve } from "node:path"
import {
  analyzeTaskDocument,
  parseTaskDocument,
  resolveTask,
  TaskDocumentParseError,
  TaskResolutionError,
  type Platform,
  type ResolveContext,
  type ResolvedTaskPlan,
  type TaskAnalysis,
  type TaskAnalysisError,
  type TaskRecord,
} from "vscode-tasks-engine"

import {
  VscodeTaskErrorCodes,
  type InspectVscodeTasksRequest,
  type InspectVscodeTasksResponse,
  type ResolvedVscodeTaskPlan,
  type ResolveVscodeTaskRequest,
  type ResolveVscodeTaskResponse,
  type VscodeTaskAnalysisError,
  type VscodeTaskSummary,
} from "../schema.ts"
import { createVscodeTaskIpcError } from "./ipc-error.ts"

export type LoadedVscodeTasks = {
  analysis: TaskAnalysis
  context: ResolveContext
  inspection: InspectVscodeTasksResponse
}

export async function inspectVscodeTasks(
  request: InspectVscodeTasksRequest,
): Promise<InspectVscodeTasksResponse> {
  return (await loadVscodeTasks(request)).inspection
}

export async function resolveVscodeTask(
  request: ResolveVscodeTaskRequest,
): Promise<ResolveVscodeTaskResponse> {
  const loaded = await loadVscodeTasks(request)
  return {
    task: resolveLoadedVscodeTask(loaded, request.label),
  }
}

export async function loadVscodeTasks(
  request: InspectVscodeTasksRequest,
): Promise<LoadedVscodeTasks> {
  const workspaceRoot = resolve(request.workspaceRoot)
  const sourcePath = resolve(workspaceRoot, ".vscode", "tasks.json")
  let text: string

  try {
    text = await readFile(sourcePath, "utf8")
  } catch (error) {
    if (isNodeError(error) && error.code === "ENOENT") {
      throw createVscodeTaskIpcError(VscodeTaskErrorCodes.DocumentNotFound, { sourcePath })
    }
    throw createVscodeTaskIpcError(VscodeTaskErrorCodes.DocumentUnreadable, { sourcePath })
  }

  let analysis: TaskAnalysis
  try {
    analysis = analyzeTaskDocument(parseTaskDocument(text, sourcePath))
  } catch (error) {
    if (error instanceof TaskDocumentParseError) {
      throw createVscodeTaskIpcError(VscodeTaskErrorCodes.DocumentInvalid, {
        sourcePath,
        diagnostics: error.details,
      })
    }
    throw error
  }

  return {
    analysis,
    context: createResolveContext(workspaceRoot),
    inspection: {
      workspaceRoot,
      sourcePath,
      version: analysis.document.version ?? null,
      tasks: analysis.tasks.map(toTaskSummary),
      errors: analysis.errors.map(toAnalysisError),
    },
  }
}

export function resolveLoadedVscodeTask(
  loaded: LoadedVscodeTasks,
  label: string,
): ResolvedVscodeTaskPlan {
  try {
    return toResolvedTaskPlan(resolveTask(loaded.analysis, label, loaded.context))
  } catch (error) {
    if (error instanceof TaskResolutionError) {
      throw createVscodeTaskIpcError(VscodeTaskErrorCodes.TaskUnavailable, { label })
    }
    throw error
  }
}

export function createResolveContext(workspaceRoot: string): ResolveContext {
  return {
    workspaceRoot,
    cwd: workspaceRoot,
    platform: toVscodeTaskPlatform(process.platform),
    env: process.env,
  }
}

export function toVscodeTaskPlatform(platform: NodeJS.Platform): Platform {
  if (platform === "win32") {
    return "windows"
  }
  if (platform === "darwin") {
    return "osx"
  }
  return "linux"
}

function toTaskSummary(task: TaskRecord): VscodeTaskSummary {
  return {
    id: task.id,
    label: task.label ?? null,
    supported: task.supported,
    kind: task.kind ?? null,
    hidden: task.hidden,
    group: task.group ?? null,
    dependsOn: task.dependsOn,
    dependsOrder: task.dependsOrder,
    issues: task.issues,
    unsupportedReason: task.unsupportedReason ?? null,
  }
}

function toAnalysisError(error: TaskAnalysisError): VscodeTaskAnalysisError {
  return {
    code: error.code,
    message: error.message,
    taskId: error.taskId ?? null,
    label: error.label ?? null,
  }
}

function toResolvedTaskPlan(task: ResolvedTaskPlan): ResolvedVscodeTaskPlan {
  return {
    label: task.label,
    kind: task.kind,
    command: task.command ?? null,
    args: task.args,
    cwd: task.cwd,
    env: task.env,
    shell: task.shell
      ? {
          executable: task.shell.executable ?? null,
          args: task.shell.args ?? null,
        }
      : null,
    hidden: task.hidden,
    group: task.group ?? null,
    dependsOn: task.dependsOn,
    dependsOrder: task.dependsOrder,
    dependencies: task.dependencies.map(toResolvedTaskPlan),
  }
}

function isNodeError(error: unknown): error is NodeJS.ErrnoException {
  return error instanceof Error && "code" in error
}
