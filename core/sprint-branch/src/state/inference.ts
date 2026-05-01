import path from "node:path"
import { autocomplete, isCancel } from "@clack/prompts"

import { getCurrentBranch, resolveRepositoryRoot } from "../git/repository"
import type { SprintContext, SprintDiagnostic } from "../types"
import { parseSprintBranchName, validateSprintName } from "./branches"
import { findSprintStateFiles, readSprintStateFile } from "./io"
import { sprintStateDisplayPath, sprintStatePath } from "./paths"

/** Error raised when the current sprint cannot be inferred safely. */
export class SprintInferenceError extends Error {
  diagnostics: SprintDiagnostic[]

  constructor(message: string, diagnostics: SprintDiagnostic[] = []) {
    super(message)
    this.name = "SprintInferenceError"
    this.diagnostics = diagnostics
  }
}

/** Inputs that allow commands to infer or interactively select the active sprint. */
export type SprintInferenceInput = {
  cwd: string
  sprint?: string
  interactive?: boolean
}

/** Infers the active sprint from explicit input or strong local context. */
export async function inferSprintContext(input: SprintInferenceInput) {
  const rootDir = await resolveRepositoryRoot(input.cwd)
  const currentBranch = await getCurrentBranch(rootDir)

  if (input.sprint) {
    assertValidSprintName(input.sprint)
    return await buildContext(rootDir, input.sprint, currentBranch, "explicit --sprint")
  }

  if (currentBranch) {
    const parsedBranch = parseSprintBranchName(currentBranch)
    if (parsedBranch) {
      return await buildContext(
        rootDir,
        parsedBranch.sprint,
        currentBranch,
        `current branch ${currentBranch}`,
      )
    }
  }

  const pathSprint = inferSprintFromPath(rootDir, input.cwd)
  if (pathSprint) {
    return await buildContext(
      rootDir,
      pathSprint,
      currentBranch,
      `working directory sprints/${pathSprint}`,
    )
  }

  const candidates = await sprintCandidatesForSelection(
    rootDir,
    await findSprintStateFiles(rootDir),
  )
  if (candidates.length > 0) {
    if (canPromptForSprint(input)) {
      const selected = await autocomplete({
        message: "Select sprint",
        placeholder: "Type to filter sprints...",
        options: candidates.map((candidate) => ({
          value: candidate.sprint,
          label: candidate.sprint,
          hint: candidate.relativePath,
        })),
      })

      if (isCancel(selected)) {
        throw new SprintInferenceError("Sprint selection cancelled.", [
          {
            severity: "error",
            code: "sprint_selection_cancelled",
            message: "Sprint selection cancelled.",
          },
        ])
      }

      return await buildContext(rootDir, selected, currentBranch, "interactive sprint selection")
    }

    throw new SprintInferenceError(
      "No sprint selected. Pass --sprint <name> or run from a sprint branch or sprints/<name>.",
      [
        {
          severity: "error",
          code: "sprint_selection_required",
          message: "No sprint could be inferred from --sprint, a sprint branch, or sprints/<name>.",
          suggestion: "Pass --sprint <name>.",
        },
        ...candidates.map((candidate) => ({
          severity: "info" as const,
          code: "available_sprint_state",
          message: candidate.relativePath,
          suggestion: `--sprint ${candidate.sprint}`,
        })),
      ],
    )
  }

  throw new SprintInferenceError(
    "Unable to infer the current sprint from --sprint, a sprint branch, or sprints/<name>.",
    [
      {
        severity: "error",
        code: "missing_sprint_state",
        message:
          "Looked for explicit --sprint, a sprint branch name, a sprints/<name> working directory, and Git metadata sprint-branch/*/state.json.",
      },
    ],
  )
}

function assertValidSprintName(sprint: string) {
  const diagnostics = validateSprintName(sprint)
  if (diagnostics.length > 0) {
    throw new SprintInferenceError(`Invalid sprint name: ${sprint}`, diagnostics)
  }
}

async function buildContext(
  rootDir: string,
  sprint: string,
  currentBranch: string | null,
  inferredFrom: string,
) {
  assertValidSprintName(sprint)
  const statePath = await sprintStatePath(rootDir, sprint)
  return {
    rootDir,
    sprint,
    statePath,
    stateRelativePath: sprintStateDisplayPath(sprint),
    currentBranch,
    inferredFrom,
  } satisfies SprintContext
}

function inferSprintFromPath(rootDir: string, cwd: string) {
  const relative = path.relative(path.join(rootDir, "sprints"), cwd)
  if (relative === "" || relative.startsWith("..") || path.isAbsolute(relative)) {
    return null
  }

  const [sprint] = relative.split(path.sep)
  return sprint || null
}

async function sprintCandidatesForSelection(rootDir: string, stateFiles: string[]) {
  const candidates = []
  for (const statePath of stateFiles) {
    try {
      const parsed = await readSprintStateFile(statePath)
      if (parsed.state?.visibility === "active") {
        candidates.push({
          sprint: path.basename(path.dirname(statePath)),
          relativePath: statePathForDisplay(rootDir, statePath),
        })
      }
    } catch {
      continue
    }
  }

  return candidates.sort((left, right) => left.sprint.localeCompare(right.sprint))
}

function canPromptForSprint(input: SprintInferenceInput) {
  return input.interactive !== false && Boolean(process.stdin.isTTY && process.stdout.isTTY)
}

function statePathForDisplay(rootDir: string, statePath: string) {
  const parts = statePath.split(path.sep)
  const rootIndex = parts.lastIndexOf("sprint-branch")
  if (rootIndex !== -1) {
    return path.join(".git", ...parts.slice(rootIndex))
  }
  return path.relative(rootDir, statePath)
}
