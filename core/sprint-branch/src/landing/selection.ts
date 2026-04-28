import path from "node:path"
import { isCancel, select } from "@clack/prompts"

import { parseSprintBranchName, validateSprintName } from "../state/branches"
import { findSprintStateFiles, readSprintStateFile } from "../state/io"
import { sprintStatePath } from "../state/paths"
import type { SprintDiagnostic } from "../types"
import type { HumanCommandInput, SprintCandidate } from "./types"

/** Resolves the sprint state a human landing command should operate on. */
export async function resolveSprintCandidate(
  rootDir: string,
  input: HumanCommandInput,
  currentBranch: string | null,
  diagnostics: SprintDiagnostic[],
) {
  if (input.sprint) {
    return readExplicitCandidate(rootDir, input.sprint, diagnostics)
  }

  const candidates = await readSprintCandidates(rootDir)
  const inferred = inferCandidate(rootDir, input.cwd, currentBranch, candidates)
  if (inferred) {
    return inferred
  }
  if (candidates.length === 0) {
    diagnostics.push({
      severity: "error",
      code: "missing_sprint_state",
      message: "No sprints/*/.sprint-branch-state.json files were found.",
    })
    return null
  }
  if (candidates.length === 1) {
    return candidates[0]
  }
  if (input.json || !process.stdin.isTTY || !process.stdout.isTTY) {
    diagnostics.push({
      severity: "error",
      code: "ambiguous_sprint",
      message: "Multiple sprints are available. Pass the sprint name as an argument.",
    })
    return null
  }

  const selected = await select({
    message: "Select sprint",
    options: candidates.map((candidate) => ({
      value: candidate.sprint,
      label: candidate.sprint,
      hint: candidate.reviewBranch,
    })),
  })

  if (isCancel(selected)) {
    diagnostics.push({
      severity: "error",
      code: "sprint_selection_cancelled",
      message: "Sprint selection cancelled.",
    })
    return null
  }

  return candidates.find((candidate) => candidate.sprint === selected) ?? null
}

/** Lists available sprint candidates for non-interactive diagnostics. */
export async function candidatesForOutput(rootDir: string) {
  return (await readSprintCandidates(rootDir)).map((candidate) => ({
    sprint: candidate.sprint,
    statePath: candidate.stateRelativePath,
    reviewBranch: candidate.reviewBranch,
  }))
}

async function readExplicitCandidate(
  rootDir: string,
  sprint: string,
  diagnostics: SprintDiagnostic[],
) {
  const validation = validateSprintName(sprint)
  diagnostics.push(...validation)
  if (validation.some((diagnostic) => diagnostic.severity === "error")) {
    return null
  }

  const statePath = sprintStatePath(rootDir, sprint)
  try {
    const parsed = await readSprintStateFile(statePath)
    diagnostics.push(...parsed.diagnostics)
    return parsed.state ? candidateFromState(rootDir, statePath, parsed.state) : null
  } catch (error) {
    if (isMissingFileError(error)) {
      diagnostics.push({
        severity: "error",
        code: "missing_sprint_state",
        message: `Sprint state sprints/${sprint}/.sprint-branch-state.json does not exist.`,
      })
      return null
    }
    throw error
  }
}

async function readSprintCandidates(rootDir: string) {
  const stateFiles = await findSprintStateFiles(rootDir)
  const candidates: SprintCandidate[] = []

  for (const statePath of stateFiles) {
    try {
      const parsed = await readSprintStateFile(statePath)
      if (parsed.state) {
        candidates.push(candidateFromState(rootDir, statePath, parsed.state))
      }
    } catch {
      continue
    }
  }

  return candidates.sort((left, right) => left.sprint.localeCompare(right.sprint))
}

function inferCandidate(
  rootDir: string,
  cwd: string,
  currentBranch: string | null,
  candidates: SprintCandidate[],
) {
  if (currentBranch) {
    const branchSprint = parseSprintBranchName(currentBranch)?.sprint
    const candidate = candidates.find((entry) => entry.sprint === branchSprint)
    if (candidate) {
      return candidate
    }
  }

  const pathSprint = inferSprintFromPath(rootDir, cwd)
  return candidates.find((entry) => entry.sprint === pathSprint) ?? null
}

function candidateFromState(
  rootDir: string,
  statePath: string,
  state: SprintCandidate["state"],
): SprintCandidate {
  return {
    sprint: state.sprint,
    stateRelativePath: path.relative(rootDir, statePath),
    reviewBranch: state.branches.review,
    state,
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

function isMissingFileError(error: unknown) {
  return (
    typeof error === "object" &&
    error !== null &&
    "code" in error &&
    (error as { code?: unknown }).code === "ENOENT"
  )
}
