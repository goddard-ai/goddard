/** File-backed stop control for long-running sprint sync processes. */
import { createHash, randomUUID } from "node:crypto"
import * as fs from "node:fs/promises"
import path from "node:path"
import { setTimeout as sleep } from "node:timers/promises"

import { resolveGitCommonPath, resolveRepositoryRoot } from "./git/repository"
import { sprintStateRoot } from "./state/paths"
import type { SprintDiagnostic, SprintSyncStopReport } from "./types"

const syncControlPollMs = 100
const syncControlSchemaVersion = 1
const syncStopReason = "sprint-branch stop-sync"
const syncReplaceWaitMs = 5000

/** File contents owned by one running sync process. */
type SyncRunControlFile = {
  schemaVersion: 1
  runId: string
  pid: number
  cwd: string
  startedAt: string
  stopRequested: boolean
  stopRequestedAt?: string
  stopReason?: string
}

/** A registered sync process that still appears to be alive. */
type RunningSyncControlFile = SyncRunControlFile & {
  controlPath: string
}

/** Registers a running sync so another shell in the same directory can stop it. */
export async function createSprintSyncStopControl(input: { cwd: string; signal?: AbortSignal }) {
  const cwd = await fs.realpath(input.cwd)
  const controlDir = await syncControlDir(input.cwd, cwd)
  await fs.mkdir(controlDir, { recursive: true })

  const runId = `${Date.now()}-${process.pid}-${randomUUID()}`
  const controlPath = path.join(controlDir, `${runId}.json`)
  const controller = new AbortController()

  await fs.writeFile(
    controlPath,
    formatSyncRunControlFile({
      schemaVersion: syncControlSchemaVersion,
      runId,
      pid: process.pid,
      cwd,
      startedAt: new Date().toISOString(),
      stopRequested: false,
    }),
  )

  const abortFromParent = () => {
    if (!controller.signal.aborted) {
      controller.abort(input.signal?.reason)
    }
  }

  if (input.signal?.aborted) {
    abortFromParent()
  } else {
    input.signal?.addEventListener("abort", abortFromParent, { once: true })
  }

  let checking = false
  const checkForStop = async () => {
    if (checking || controller.signal.aborted) {
      return
    }

    checking = true
    try {
      const control = await readSyncRunControlFile(controlPath)
      if (control?.stopRequested && !controller.signal.aborted) {
        controller.abort(syncStopReason)
      }
    } finally {
      checking = false
    }
  }

  const interval = setInterval(() => {
    void checkForStop()
  }, syncControlPollMs)
  void checkForStop()

  return {
    signal: controller.signal,
    cleanup: async () => {
      clearInterval(interval)
      input.signal?.removeEventListener("abort", abortFromParent)
      await fs.rm(controlPath, { force: true })
      await removeEmptyDirectory(controlDir)
    },
  }
}

/** Requests that every registered sync in the exact current working directory stop. */
export async function requestSprintSyncStop(input: { cwd: string }) {
  const running = await readRunningSyncControls(input)
  const syncs = []

  for (const control of running.controls) {
    const { controlPath, ...storedControl } = control
    const stopped = {
      ...storedControl,
      stopRequested: true,
      stopRequestedAt: new Date().toISOString(),
      stopReason: syncStopReason,
    }

    if (await overwriteExistingSyncRunControlFile(controlPath, stopped)) {
      syncs.push({
        runId: control.runId,
        pid: control.pid,
        startedAt: control.startedAt,
      })
    }
  }

  return {
    ok: true,
    command: "stop-sync",
    cwd: running.cwd,
    stopped: syncs.length,
    syncs,
    diagnostics: running.diagnostics,
  } satisfies SprintSyncStopReport
}

/** Returns live sync processes registered from this exact working directory. */
export async function findRunningSprintSyncs(input: { cwd: string }) {
  const running = await readRunningSyncControls(input)
  return {
    cwd: running.cwd,
    syncs: running.controls.map((control) => ({
      runId: control.runId,
      pid: control.pid,
      startedAt: control.startedAt,
    })),
    diagnostics: running.diagnostics,
  }
}

/** Stops existing same-directory syncs and waits for their normal cleanup. */
export async function replaceRunningSprintSyncs(input: { cwd: string; timeoutMs?: number }) {
  const stop = await requestSprintSyncStop(input)
  if (stop.stopped === 0) {
    return stop
  }

  const deadline = Date.now() + (input.timeoutMs ?? syncReplaceWaitMs)
  let latest = await findRunningSprintSyncs(input)
  while (latest.syncs.length > 0 && Date.now() < deadline) {
    await sleep(syncControlPollMs)
    latest = await findRunningSprintSyncs(input)
  }

  if (latest.syncs.length === 0) {
    return {
      ...stop,
      diagnostics: [...stop.diagnostics, ...latest.diagnostics],
    } satisfies SprintSyncStopReport
  }

  return {
    ...stop,
    ok: false,
    diagnostics: [
      ...stop.diagnostics,
      ...latest.diagnostics,
      {
        severity: "error",
        code: "sync_replace_timeout",
        message: `${latest.syncs.length} sprint-branch sync ${
          latest.syncs.length === 1 ? "process has" : "processes have"
        } not stopped yet for ${latest.cwd}.`,
        suggestion: "Wait for the existing sync cleanup to finish, then retry.",
      },
    ],
  } satisfies SprintSyncStopReport
}

/** Diagnostics used when a new sync would conflict with an existing same-cwd sync. */
export function runningSprintSyncDiagnostics(input: {
  cwd: string
  syncs: Array<{ pid: number; startedAt: string }>
}) {
  return [
    {
      severity: "error" as const,
      code: "sync_already_running",
      message: `A sprint-branch sync is already running in ${input.cwd}.`,
      suggestion:
        "Run sprint-branch sync --replace to stop it before starting a new sync, or run sprint-branch stop-sync.",
    },
    ...input.syncs.map((sync) => ({
      severity: "info" as const,
      code: "running_sync_process",
      message: `Process ${sync.pid} started at ${sync.startedAt}.`,
    })),
  ] satisfies SprintDiagnostic[]
}

/** Formats the stop-sync report for human readers. */
export function formatSprintSyncStopReport(report: SprintSyncStopReport) {
  const lines =
    report.stopped > 0
      ? [
          `Requested stop for ${report.stopped} sprint-branch sync ${
            report.stopped === 1 ? "process" : "processes"
          } in ${report.cwd}.`,
        ]
      : [`No running sprint-branch sync found for ${report.cwd}.`]

  if (report.diagnostics.length > 0) {
    lines.push("", ...formatDiagnostics(report.diagnostics))
  }

  return lines.join("\n")
}

/** Resolves the shared Git-private directory for one real working directory. */
async function syncControlDir(startDir: string, cwd: string) {
  const rootDir = await resolveRepositoryRoot(startDir)
  return resolveGitCommonPath(rootDir, path.join(sprintStateRoot, "sync", syncControlKey(cwd)))
}

/** Reads running sync controls and removes records whose processes are gone. */
async function readRunningSyncControls(input: { cwd: string }) {
  const cwd = await fs.realpath(input.cwd)
  const controlDir = await syncControlDir(input.cwd, cwd)
  const controlPaths = await listSyncRunControlPaths(controlDir)
  const diagnostics: SprintDiagnostic[] = []
  const controls: RunningSyncControlFile[] = []

  for (const controlPath of controlPaths) {
    const control = await readSyncRunControlFile(controlPath)
    if (!control) {
      diagnostics.push({
        severity: "warning",
        code: "invalid_sync_control",
        message: `Ignored invalid sync control file ${path.basename(controlPath)}.`,
      })
      continue
    }

    if (control.cwd !== cwd) {
      continue
    }

    if (!isProcessRunning(control.pid)) {
      await fs.rm(controlPath, { force: true })
      diagnostics.push({
        severity: "info",
        code: "stale_sync_control",
        message: `Removed stale sync control for process ${control.pid}.`,
      })
      continue
    }

    controls.push({ ...control, controlPath })
  }

  return { cwd, controls, diagnostics }
}

/** Reads all currently registered sync control files for a working directory key. */
async function listSyncRunControlPaths(controlDir: string) {
  try {
    const entries = await fs.readdir(controlDir, { withFileTypes: true })
    return entries
      .filter((entry) => entry.isFile() && entry.name.endsWith(".json"))
      .map((entry) => path.join(controlDir, entry.name))
  } catch (error) {
    if (isMissingFileError(error)) {
      return []
    }
    throw error
  }
}

/** Reads a control file, tolerating concurrent cleanup or writes. */
async function readSyncRunControlFile(controlPath: string) {
  try {
    return normalizeSyncRunControlFile(JSON.parse(await fs.readFile(controlPath, "utf-8")))
  } catch (error) {
    if (isMissingFileError(error) || error instanceof SyntaxError) {
      return null
    }
    throw error
  }
}

/** Overwrites an existing control file without recreating a run that already exited. */
async function overwriteExistingSyncRunControlFile(
  controlPath: string,
  control: SyncRunControlFile,
) {
  let file: fs.FileHandle | null = null
  try {
    file = await fs.open(controlPath, "r+")
    await file.truncate(0)
    await file.writeFile(formatSyncRunControlFile(control))
    return true
  } catch (error) {
    if (isMissingFileError(error)) {
      return false
    }
    throw error
  } finally {
    await file?.close()
  }
}

/** Normalizes untrusted JSON into the control shape used by running syncs. */
function normalizeSyncRunControlFile(value: unknown) {
  if (typeof value !== "object" || value === null) {
    return null
  }

  const candidate = value as Partial<SyncRunControlFile>
  if (
    candidate.schemaVersion !== syncControlSchemaVersion ||
    typeof candidate.runId !== "string" ||
    typeof candidate.pid !== "number" ||
    typeof candidate.cwd !== "string" ||
    typeof candidate.startedAt !== "string" ||
    typeof candidate.stopRequested !== "boolean"
  ) {
    return null
  }

  return {
    schemaVersion: syncControlSchemaVersion,
    runId: candidate.runId,
    pid: candidate.pid,
    cwd: candidate.cwd,
    startedAt: candidate.startedAt,
    stopRequested: candidate.stopRequested,
    ...(typeof candidate.stopRequestedAt === "string"
      ? { stopRequestedAt: candidate.stopRequestedAt }
      : {}),
    ...(typeof candidate.stopReason === "string" ? { stopReason: candidate.stopReason } : {}),
  } satisfies SyncRunControlFile
}

function formatSyncRunControlFile(control: SyncRunControlFile) {
  return `${JSON.stringify(control, null, 2)}\n`
}

function syncControlKey(cwd: string) {
  return createHash("sha256").update(cwd).digest("hex").slice(0, 32)
}

function isProcessRunning(pid: number) {
  try {
    process.kill(pid, 0)
    return true
  } catch (error) {
    return !isErrnoCode(error, "ESRCH")
  }
}

async function removeEmptyDirectory(directory: string) {
  try {
    await fs.rmdir(directory)
  } catch (error) {
    if (!isErrnoCode(error, "ENOENT") && !isErrnoCode(error, "ENOTEMPTY")) {
      throw error
    }
  }
}

function formatDiagnostics(diagnostics: SprintDiagnostic[]) {
  return diagnostics.flatMap((diagnostic) => [
    `[${diagnostic.severity}] ${diagnostic.code}: ${diagnostic.message}`,
    ...(diagnostic.suggestion ? [`suggestion: ${diagnostic.suggestion}`] : []),
  ])
}

function isMissingFileError(error: unknown) {
  return isErrnoCode(error, "ENOENT")
}

function isErrnoCode(error: unknown, code: string) {
  return error instanceof Error && "code" in error && (error as NodeJS.ErrnoException).code === code
}
