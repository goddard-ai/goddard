import * as fs from "node:fs/promises"
import path from "node:path"

import { resolveGitCommonPath } from "./git/repository"
import { inferSprintContext, type SprintInferenceInput } from "./state/inference"
import { readSprintStateFile } from "./state/io"
import { sprintStateRoot } from "./state/paths"
import type { SprintBranchState, SprintDiagnostic } from "./types"

const backupDirectoryName = "sprints-backup"
const backupFilesDirectoryName = "files"
const backupManifestFileName = "manifest.json"

/** Report returned after planning or restoring a sprint folder backup. */
export type SprintRestoreReport = {
  ok: boolean
  command: "restore-sprint"
  dryRun: boolean
  executed: boolean
  sprint: string
  currentBranch: string | null
  backupPath: string
  restoredPath: string
  backedUpAt: string | null
  diagnostics: SprintDiagnostic[]
}

/** Refreshes the latest Git-private backup for a sprint folder when it exists. */
export async function backupSprintFolder(
  rootDir: string,
  state: Pick<SprintBranchState, "sprint" | "sprintWorktreeRoot">,
) {
  const sourcePath = path.join(state.sprintWorktreeRoot, "sprints", state.sprint)
  if (!(await directoryExists(sourcePath))) {
    return
  }

  const backupPath = await sprintBackupPath(rootDir, state.sprint)
  const tempPath = `${backupPath}.${process.pid}.${Date.now()}.tmp`
  await fs.rm(tempPath, { recursive: true, force: true })
  await fs.mkdir(tempPath, { recursive: true })
  await fs.cp(sourcePath, path.join(tempPath, backupFilesDirectoryName), { recursive: true })
  await fs.writeFile(
    path.join(tempPath, backupManifestFileName),
    `${JSON.stringify(
      {
        sprint: state.sprint,
        backedUpAt: new Date().toISOString(),
        sourcePath,
      },
      null,
      2,
    )}\n`,
  )
  await fs.rm(backupPath, { recursive: true, force: true })
  await fs.rename(tempPath, backupPath)
}

/** Deletes the latest backup for a sprint folder. */
export async function removeSprintFolderBackup(rootDir: string, sprint: string) {
  await fs.rm(await sprintBackupPath(rootDir, sprint), { recursive: true, force: true })
}

/** Restores a missing sprint folder from the latest Git-private backup. */
export async function runRestoreSprint(
  input: SprintInferenceInput & { dryRun: boolean; force?: boolean },
) {
  const context = await inferSprintContext(input)
  const diagnostics: SprintDiagnostic[] = []
  const backupPath = await sprintBackupPath(context.rootDir, context.sprint)
  const manifest = await readBackupManifest(backupPath, diagnostics)
  const parsed = await readOptionalSprintState(context.statePath, diagnostics)
  const targetRoot = parsed.state?.sprintWorktreeRoot ?? context.rootDir
  const restoredPath = path.join(targetRoot, "sprints", context.sprint)
  const backupFilesPath = path.join(backupPath, backupFilesDirectoryName)

  if (!(await directoryExists(backupFilesPath))) {
    diagnostics.push({
      severity: "error",
      code: "sprint_backup_missing",
      message: `No sprint backup exists at ${sprintBackupDisplayPath(context.sprint)}.`,
    })
  }
  if ((await pathExists(restoredPath)) && !input.force) {
    diagnostics.push({
      severity: "error",
      code: "sprint_folder_exists",
      message: `Sprint folder ${path.relative(targetRoot, restoredPath)} already exists.`,
      suggestion:
        "Pass --force only after preserving or intentionally replacing the existing folder.",
    })
  }

  const report = {
    ok: !diagnostics.some((diagnostic) => diagnostic.severity === "error"),
    command: "restore-sprint" as const,
    dryRun: input.dryRun,
    executed: false,
    sprint: context.sprint,
    currentBranch: context.currentBranch,
    backupPath: sprintBackupDisplayPath(context.sprint),
    restoredPath: path.relative(targetRoot, restoredPath),
    backedUpAt: manifest?.backedUpAt ?? null,
    diagnostics,
  } satisfies SprintRestoreReport

  if (input.dryRun || !report.ok) {
    return report
  }

  await fs.rm(restoredPath, { recursive: true, force: true })
  await fs.mkdir(path.dirname(restoredPath), { recursive: true })
  await fs.cp(backupFilesPath, restoredPath, { recursive: true })
  return { ...report, executed: true } satisfies SprintRestoreReport
}

/** Formats restore output for human-oriented CLI output. */
export function formatSprintRestoreReport(report: SprintRestoreReport) {
  const lines = [
    `${report.dryRun ? "Dry run" : report.executed ? "Executed" : "Planned"}: restore-sprint`,
    `Sprint: ${report.sprint}`,
    `Current branch: ${report.currentBranch ?? "detached HEAD"}`,
    `Backup: ${report.backupPath}`,
    `Restores to: ${report.restoredPath}`,
    `Backed up at: ${report.backedUpAt ?? "unknown"}`,
  ]

  if (report.diagnostics.length > 0) {
    lines.push("", "Diagnostics:")
    for (const diagnostic of report.diagnostics) {
      lines.push(`  [${diagnostic.severity}] ${diagnostic.code}: ${diagnostic.message}`)
      if (diagnostic.suggestion) {
        lines.push(`    suggestion: ${diagnostic.suggestion}`)
      }
    }
  }

  return lines.join("\n")
}

/** Returns the Git-private path for a sprint folder backup. */
export async function sprintBackupPath(rootDir: string, sprint: string) {
  return resolveGitCommonPath(rootDir, path.join(sprintStateRoot, sprint, backupDirectoryName))
}

/** Returns a stable display path for a sprint folder backup. */
export function sprintBackupDisplayPath(sprint: string) {
  return path.posix.join(".git", sprintStateRoot, sprint, backupDirectoryName)
}

async function readOptionalSprintState(statePath: string, diagnostics: SprintDiagnostic[]) {
  try {
    const parsed = await readSprintStateFile(statePath)
    diagnostics.push(...parsed.diagnostics)
    return parsed
  } catch (error) {
    if (!isMissingFileError(error)) {
      throw error
    }
    diagnostics.push({
      severity: "warning",
      code: "missing_sprint_state",
      message: "Private sprint state is missing; restoring into the current worktree.",
    })
    return { state: null, diagnostics: [] }
  }
}

async function readBackupManifest(backupPath: string, diagnostics: SprintDiagnostic[]) {
  try {
    const text = await fs.readFile(path.join(backupPath, backupManifestFileName), "utf-8")
    const value = JSON.parse(text) as { backedUpAt?: unknown }
    return { backedUpAt: typeof value.backedUpAt === "string" ? value.backedUpAt : null }
  } catch (error) {
    if (isMissingFileError(error)) {
      return null
    }
    diagnostics.push({
      severity: "warning",
      code: "sprint_backup_manifest_invalid",
      message: "Sprint backup manifest could not be read; restore can still use backup files.",
    })
    return null
  }
}

async function pathExists(targetPath: string) {
  try {
    await fs.access(targetPath)
    return true
  } catch (error) {
    if (isMissingFileError(error)) {
      return false
    }
    throw error
  }
}

async function directoryExists(targetPath: string) {
  try {
    return (await fs.stat(targetPath)).isDirectory()
  } catch (error) {
    if (isMissingFileError(error)) {
      return false
    }
    throw error
  }
}

function isMissingFileError(error: unknown) {
  return (
    typeof error === "object" &&
    error !== null &&
    "code" in error &&
    (error as { code?: unknown }).code === "ENOENT"
  )
}
