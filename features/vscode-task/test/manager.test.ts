import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { IpcClientError } from "@goddard-ai/ipc"
import { DaemonTerminalProcessService } from "@goddard-ai/terminal/daemon"
import { afterEach, describe, expect, test } from "bun:test"

import { VscodeTaskManager } from "../src/daemon/manager.ts"
import { VscodeTaskErrorCodes, type VscodeTaskDaemonEvent } from "../src/schema.ts"

const tempDirectories: string[] = []
const managers: VscodeTaskManager[] = []
const terminals: DaemonTerminalProcessService[] = []

afterEach(async () => {
  for (const manager of managers.splice(0)) {
    manager.closeAll()
  }
  for (const terminal of terminals.splice(0)) {
    terminal.closeAll()
  }
  await Promise.all(
    tempDirectories.splice(0).map((path) => rm(path, { recursive: true, force: true })),
  )
})

describe("daemon workspace task execution", () => {
  test("inspects unsupported tasks and analysis errors without hiding them", async () => {
    const workspaceRoot = await createWorkspace({
      version: "2.0.0",
      tasks: [
        {
          label: "build",
          type: "process",
          command: process.execPath,
          args: ["--version"],
        },
        {
          label: "extension task",
          type: "npm",
          script: "build",
        },
        {
          label: "broken dependencies",
          dependsOn: "missing",
        },
      ],
    })
    const { manager } = createManager()

    const result = await manager.inspect({ workspaceRoot })

    expect(result.workspaceRoot).toBe(workspaceRoot)
    expect(result.tasks.map(({ label, supported }) => ({ label, supported }))).toEqual([
      { label: "build", supported: true },
      { label: "extension task", supported: false },
      { label: "broken dependencies", supported: false },
    ])
    expect(result.tasks[1]?.unsupportedReason).not.toBeNull()
    expect(result.errors.some((error) => error.code === "unknown_dependency")).toBe(true)
  })

  test("resolves and runs process and shell dependencies in sequence through PTYs", async () => {
    const processMarker = "__goddard_process_task__"
    const shellMarker = "__goddard_shell_task__"
    const workspaceRoot = await createWorkspace({
      version: "2.0.0",
      tasks: [
        {
          label: "prepare",
          type: "process",
          command: process.execPath,
          args: ["-e", `console.log("${processMarker}")`],
        },
        {
          label: "report",
          type: "shell",
          command: "printf",
          args: [`${shellMarker}\\n`],
        },
        {
          label: "build",
          dependsOn: ["prepare", "report"],
          dependsOrder: "sequence",
        },
      ],
    })
    const { manager, events } = createManager()
    const { connectionId } = manager.connect()
    manager.streamConnected(connectionId)

    const preview = await manager.resolve({ workspaceRoot, label: "build" })
    expect(preview.task.kind).toBe("compound")
    expect(preview.task.dependencies.map((task) => task.label)).toEqual(["prepare", "report"])

    const run = await manager.run({ connectionId, workspaceRoot, label: "build" })
    const completed = await waitForEvent(
      events,
      (event) => event.type === "vscode-task.run.completed" && event.runId === run.runId,
    )

    expect(completed.type).toBe("vscode-task.run.completed")
    if (completed.type !== "vscode-task.run.completed") {
      throw new Error("Expected a completed task event.")
    }
    expect(completed.result.status).toBe("success")
    expect(
      events
        .filter((event) => event.type === "vscode-task.process.started")
        .map((event) => event.label),
    ).toEqual(["prepare", "report"])
    const output = events
      .filter((event) => event.type === "vscode-task.process.output")
      .map((event) => event.data)
      .join("")
    expect(output).toContain(processMarker)
    expect(output).toContain(shellMarker)
  })

  test("cancels active task graphs when their stream disconnects", async () => {
    const workspaceRoot = await createWorkspace({
      version: "2.0.0",
      tasks: [
        {
          label: "watch",
          type: "process",
          command: process.execPath,
          args: ["-e", "setInterval(() => {}, 1000)"],
        },
      ],
    })
    const { manager, terminal, events } = createManager()
    const { connectionId } = manager.connect()
    manager.streamConnected(connectionId)

    const run = await manager.run({ connectionId, workspaceRoot, label: "watch" })
    await waitForEvent(
      events,
      (event) => event.type === "vscode-task.process.started" && event.runId === run.runId,
    )
    manager.streamDisconnected(connectionId)

    const completed = await waitForEvent(
      events,
      (event) => event.type === "vscode-task.run.completed" && event.runId === run.runId,
    )
    if (completed.type !== "vscode-task.run.completed") {
      throw new Error("Expected a completed task event.")
    }
    expect(completed.result.status).toBe("canceled")
    expect(manager.size).toBe(0)
    await waitFor(() => terminal.size === 0)
  })

  test("cancels an explicitly addressed active run", async () => {
    const workspaceRoot = await createWorkspace({
      version: "2.0.0",
      tasks: [
        {
          label: "watch",
          type: "process",
          command: process.execPath,
          args: ["-e", "setInterval(() => {}, 1000)"],
        },
      ],
    })
    const { manager, events } = createManager()
    const { connectionId } = manager.connect()

    const run = await manager.run({ connectionId, workspaceRoot, label: "watch" })
    await waitForEvent(
      events,
      (event) => event.type === "vscode-task.process.started" && event.runId === run.runId,
    )
    expect(manager.cancel({ connectionId, runId: run.runId })).toEqual({ accepted: true })

    const completed = await waitForEvent(
      events,
      (event) => event.type === "vscode-task.run.completed" && event.runId === run.runId,
    )
    if (completed.type !== "vscode-task.run.completed") {
      throw new Error("Expected a completed task event.")
    }
    expect(completed.result.status).toBe("canceled")
  })

  test("preserves sequence fail-fast behavior", async () => {
    const workspaceRoot = await createWorkspace({
      version: "2.0.0",
      tasks: [
        {
          label: "fail",
          type: "process",
          command: process.execPath,
          args: ["-e", "process.exit(7)"],
        },
        {
          label: "must not start",
          type: "process",
          command: process.execPath,
          args: ["-e", "console.log('unexpected')"],
        },
        {
          label: "build",
          dependsOn: ["fail", "must not start"],
          dependsOrder: "sequence",
        },
      ],
    })
    const { manager, events } = createManager()
    const { connectionId } = manager.connect()

    const run = await manager.run({ connectionId, workspaceRoot, label: "build" })
    const completed = await waitForEvent(
      events,
      (event) => event.type === "vscode-task.run.completed" && event.runId === run.runId,
    )
    if (completed.type !== "vscode-task.run.completed") {
      throw new Error("Expected a completed task event.")
    }

    expect(completed.result).toMatchObject({
      status: "failed",
      failedTaskLabel: "fail",
      exitCode: 7,
    })
    expect(
      events
        .filter((event) => event.type === "vscode-task.process.started")
        .map((event) => event.label),
    ).toEqual(["fail"])
  })

  test("returns a stable structured error when tasks.json is missing", async () => {
    const workspaceRoot = await createWorkspace()
    const { manager } = createManager()

    try {
      await manager.inspect({ workspaceRoot })
      throw new Error("Expected workspace task inspection to fail.")
    } catch (error) {
      expect(error).toBeInstanceOf(IpcClientError)
      expect(error).toMatchObject({
        code: VscodeTaskErrorCodes.DocumentNotFound,
        details: { sourcePath: join(workspaceRoot, ".vscode", "tasks.json") },
      })
    }
  })
})

function createManager() {
  const terminal = new DaemonTerminalProcessService()
  const events: VscodeTaskDaemonEvent[] = []
  const manager = new VscodeTaskManager({
    terminal,
    publishEvent: (event) => events.push(event),
  })
  terminals.push(terminal)
  managers.push(manager)
  return { manager, terminal, events }
}

async function createWorkspace(tasks?: Record<string, unknown>) {
  const workspaceRoot = await mkdtemp(join(tmpdir(), "goddard-vscode-task-"))
  tempDirectories.push(workspaceRoot)
  if (tasks) {
    const vscodeDirectory = join(workspaceRoot, ".vscode")
    await mkdir(vscodeDirectory)
    await writeFile(join(vscodeDirectory, "tasks.json"), JSON.stringify(tasks), "utf8")
  }
  return workspaceRoot
}

async function waitForEvent(
  events: VscodeTaskDaemonEvent[],
  predicate: (event: VscodeTaskDaemonEvent) => boolean,
) {
  let matched: VscodeTaskDaemonEvent | undefined
  await waitFor(() => {
    matched = events.find(predicate)
    return matched !== undefined
  })
  return matched as VscodeTaskDaemonEvent
}

async function waitFor(predicate: () => boolean, timeoutMs = 5_000) {
  const deadline = Date.now() + timeoutMs
  while (!predicate()) {
    if (Date.now() >= deadline) {
      throw new Error("Timed out waiting for workspace task state.")
    }
    await Bun.sleep(10)
  }
}
