import { randomUUID } from "node:crypto"
import type { DaemonTerminalProcessService } from "@goddard-ai/terminal/daemon"
import { runTask, type TaskRun, type TaskRunEvent, type TaskRunResult } from "vscode-tasks-engine"

import {
  VscodeTaskErrorCodes,
  type InspectVscodeTasksRequest,
  type VscodeTaskCancelRequest,
  type VscodeTaskConnectionId,
  type VscodeTaskConnectResponse,
  type VscodeTaskDaemonEvent,
  type VscodeTaskMutationResponse,
  type VscodeTaskRunId,
  type VscodeTaskRunRequest,
  type VscodeTaskRunResponse,
  type VscodeTaskRunResult,
} from "../schema.ts"
import { assertLoadedVscodeTaskRunnable, inspectVscodeTasks, loadVscodeTasks } from "./document.ts"
import { createVscodeTaskHost } from "./host.ts"
import { createVscodeTaskIpcError } from "./ipc-error.ts"

type VscodeTaskConnection = {
  runs: Map<VscodeTaskRunId, TaskRun>
  streamActive: boolean
}

export type VscodeTaskManagerOptions = {
  terminal: Pick<DaemonTerminalProcessService, "spawn">
  publishEvent(event: VscodeTaskDaemonEvent): void
}

export class VscodeTaskManager {
  readonly #connections = new Map<VscodeTaskConnectionId, VscodeTaskConnection>()
  readonly #terminal: Pick<DaemonTerminalProcessService, "spawn">
  readonly #publishEvent: (event: VscodeTaskDaemonEvent) => void

  constructor(options: VscodeTaskManagerOptions) {
    this.#terminal = options.terminal
    this.#publishEvent = options.publishEvent
  }

  get size() {
    return this.#connections.size
  }

  inspect(request: InspectVscodeTasksRequest) {
    return inspectVscodeTasks(request)
  }

  connect(): VscodeTaskConnectResponse {
    const connectionId = `vstc_${randomUUID()}` as VscodeTaskConnectionId
    this.#connections.set(connectionId, {
      runs: new Map(),
      streamActive: false,
    })
    return { connectionId }
  }

  async run(request: VscodeTaskRunRequest): Promise<VscodeTaskRunResponse> {
    this.#requireConnection(request.connectionId)
    const loaded = await loadVscodeTasks(request)
    assertLoadedVscodeTaskRunnable(loaded, request.label)
    // Stream teardown may have removed ownership while the workspace file was loading.
    const connection = this.#requireConnection(request.connectionId)

    const runId = `vstr_${randomUUID()}` as VscodeTaskRunId
    this.#publishEvent({
      type: "vscode-task.run.started",
      connectionId: request.connectionId,
      runId,
      label: request.label,
    })

    const taskRun = runTask(
      loaded.analysis,
      request.label,
      loaded.context,
      createVscodeTaskHost(this.#terminal, loaded.context.platform),
      {
        onEvent: (event) => this.#publishTaskRunEvent(request.connectionId, runId, event),
      },
    )
    connection.runs.set(runId, taskRun)

    void taskRun.result.then(
      (result) => {
        this.#publishEvent({
          type: "vscode-task.run.completed",
          connectionId: request.connectionId,
          runId,
          result: normalizeRunResult(result),
        })
        connection.runs.delete(runId)
      },
      () => {
        this.#publishEvent({
          type: "vscode-task.run.failed",
          connectionId: request.connectionId,
          runId,
          code: VscodeTaskErrorCodes.RunFailed,
          details: { label: request.label },
        })
        connection.runs.delete(runId)
      },
    )

    return { runId, label: request.label }
  }

  cancel(request: VscodeTaskCancelRequest): VscodeTaskMutationResponse {
    const connection = this.#requireConnection(request.connectionId)
    const run = connection.runs.get(request.runId)
    if (!run) {
      throw createVscodeTaskIpcError(VscodeTaskErrorCodes.RunNotFound, request)
    }
    run.cancel()
    return { accepted: true }
  }

  disconnect(connectionId: VscodeTaskConnectionId): VscodeTaskMutationResponse {
    const connection = this.#requireConnection(connectionId)
    this.#closeConnection(connectionId, connection)
    return { accepted: true }
  }

  streamConnected(connectionId: VscodeTaskConnectionId) {
    const connection = this.#requireConnection(connectionId)
    if (connection.streamActive) {
      throw createVscodeTaskIpcError(VscodeTaskErrorCodes.ConnectionStreamActive, {
        connectionId,
      })
    }
    connection.streamActive = true
  }

  streamDisconnected(connectionId: VscodeTaskConnectionId) {
    const connection = this.#connections.get(connectionId)
    if (connection) {
      this.#closeConnection(connectionId, connection)
    }
  }

  closeAll() {
    for (const [connectionId, connection] of this.#connections) {
      this.#closeConnection(connectionId, connection)
    }
  }

  #publishTaskRunEvent(
    connectionId: VscodeTaskConnectionId,
    runId: VscodeTaskRunId,
    event: TaskRunEvent,
  ) {
    const base = { connectionId, runId }
    switch (event.type) {
      case "task-start":
        if (event.task.kind === "compound") {
          return
        }
        this.#publishEvent({
          ...base,
          type: "vscode-task.process.started",
          label: event.task.label,
          kind: event.task.kind,
        })
        return
      case "task-output":
        this.#publishEvent({
          ...base,
          type: "vscode-task.process.output",
          label: event.label,
          stream: event.stream,
          data:
            typeof event.chunk === "string" ? event.chunk : new TextDecoder().decode(event.chunk),
        })
        return
      case "task-exit":
        this.#publishEvent({
          ...base,
          type: "vscode-task.process.exited",
          label: event.label,
          exitCode: event.code,
          signal: event.signal ?? null,
        })
        return
      case "task-cancel":
        this.#publishEvent({
          ...base,
          type: "vscode-task.process.canceled",
          label: event.label,
          signal: event.signal ?? null,
        })
    }
  }

  #requireConnection(connectionId: string) {
    const connection = this.#connections.get(connectionId as VscodeTaskConnectionId)
    if (!connection) {
      throw createVscodeTaskIpcError(VscodeTaskErrorCodes.ConnectionNotFound, {
        connectionId,
      })
    }
    return connection
  }

  #closeConnection(connectionId: VscodeTaskConnectionId, connection: VscodeTaskConnection) {
    this.#connections.delete(connectionId)
    for (const run of connection.runs.values()) {
      run.cancel()
    }
    connection.runs.clear()
  }
}

function normalizeRunResult(result: TaskRunResult): VscodeTaskRunResult {
  return {
    status: result.status,
    label: result.label,
    failedTaskLabel: result.failedTaskLabel ?? null,
    exitCode: result.exitCode ?? null,
    signal: result.signal ?? null,
  }
}
