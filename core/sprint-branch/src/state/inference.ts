import path from "node:path"

import { getCurrentBranch, resolveRepositoryRoot } from "../git/repository"
import type { SprintContext, SprintDiagnostic } from "../types"
import { parseSprintBranchName, validateSprintName } from "./branches"
import { findSprintStateFiles, readSprintStateFile } from "./io"
import { sprintStatePath } from "./paths"

/** Error raised when the current sprint cannot be inferred safely. */
export class SprintInferenceError extends Error {
  diagnostics: SprintDiagnostic[]

  constructor(message: string, diagnostics: SprintDiagnostic[] = []) {
    super(message)
    this.name = "SprintInferenceError"
    this.diagnostics = diagnostics
  }
}

/** Infers the active sprint using the ordered rules from the workflow plan. */
export async function inferSprintContext(input: { cwd: string; sprint?: string }) {
  const rootDir = await resolveRepositoryRoot(input.cwd)
  const currentBranch = await getCurrentBranch(rootDir)

  if (input.sprint) {
    assertValidSprintName(input.sprint)
    return buildContext(rootDir, input.sprint, currentBranch, "explicit --sprint")
  }

  if (currentBranch) {
    const parsedBranch = parseSprintBranchName(currentBranch)
    if (parsedBranch) {
      return buildContext(
        rootDir,
        parsedBranch.sprint,
        currentBranch,
        `current branch ${currentBranch}`,
      )
    }
  }

  const pathSprint = inferSprintFromPath(rootDir, input.cwd)
  if (pathSprint) {
    return buildContext(
      rootDir,
      pathSprint,
      currentBranch,
      `working directory sprints/${pathSprint}`,
    )
  }

  const stateFiles = await findSprintStateFiles(rootDir)
  if (currentBranch) {
    const matchingStates = await statesReferencingBranch(rootDir, stateFiles, currentBranch)
    if (matchingStates.length === 1) {
      return buildContext(
        rootDir,
        matchingStates[0].sprint,
        currentBranch,
        `${matchingStates[0].relativePath} references ${currentBranch}`,
      )
    }
    if (matchingStates.length > 1) {
      throw new SprintInferenceError(
        `Multiple sprint state files reference ${currentBranch}. Pass --sprint <name>.`,
        matchingStates.map((match) => ({
          severity: "error",
          code: "ambiguous_sprint_state",
          message: `${match.relativePath} references ${currentBranch}.`,
          suggestion: `sprint-branch status --sprint ${match.sprint}`,
        })),
      )
    }
  }

  if (stateFiles.length === 1) {
    const sprint = path.basename(path.dirname(stateFiles[0]))
    return buildContext(rootDir, sprint, currentBranch, "only sprint state file")
  }

  if (stateFiles.length > 1) {
    throw new SprintInferenceError(
      "Multiple sprint state files exist. Pass --sprint <name>.",
      stateFiles.map((statePath) => {
        const sprint = path.basename(path.dirname(statePath))
        return {
          severity: "error",
          code: "ambiguous_sprint_state",
          message: path.relative(rootDir, statePath),
          suggestion: `sprint-branch status --sprint ${sprint}`,
        }
      }),
    )
  }

  throw new SprintInferenceError(
    "Unable to infer the current sprint from branch, path, branch references, or state files.",
    [
      {
        severity: "error",
        code: "missing_sprint_state",
        message:
          "Looked for a sprint branch name, a sprints/<name> working directory, and sprints/*/.sprint-branch-state.json.",
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

function buildContext(
  rootDir: string,
  sprint: string,
  currentBranch: string | null,
  inferredFrom: string,
): SprintContext {
  assertValidSprintName(sprint)
  const statePath = sprintStatePath(rootDir, sprint)
  return {
    rootDir,
    sprint,
    statePath,
    stateRelativePath: path.relative(rootDir, statePath),
    currentBranch,
    inferredFrom,
  }
}

function inferSprintFromPath(rootDir: string, cwd: string) {
  const relative = path.relative(path.join(rootDir, "sprints"), cwd)
  if (relative === "" || relative.startsWith("..") || path.isAbsolute(relative)) {
    return null
  }

  const [sprint] = relative.split(path.sep)
  return sprint || null
}

async function statesReferencingBranch(rootDir: string, stateFiles: string[], branch: string) {
  const matches: Array<{ sprint: string; relativePath: string }> = []

  for (const statePath of stateFiles) {
    try {
      const parsed = await readSprintStateFile(statePath)
      if (!parsed.state) {
        continue
      }
      if (Object.values(parsed.state.branches).includes(branch)) {
        matches.push({
          sprint: parsed.state.sprint,
          relativePath: path.relative(rootDir, statePath),
        })
      }
    } catch {
      continue
    }
  }

  return matches
}
