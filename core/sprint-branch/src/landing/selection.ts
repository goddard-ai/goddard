import path from "node:path"
import { autocomplete, isCancel } from "@clack/prompts"

import { parseSprintBranchName, validateSprintName } from "../state/branches"
import { findSprintStateFiles, readSprintStateFile } from "../state/io"
import { sprintStateDisplayPath, sprintStatePath } from "../state/paths"
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

  const candidates = await readSprintCandidates(rootDir, true)
  const inferred = inferCandidate(rootDir, input.cwd, currentBranch, candidates)
  const activeCandidates = candidates.filter((candidate) => candidate.state.visibility === "active")
  if (inferred) {
    return inferred
  }
  if (activeCandidates.length === 0) {
    diagnostics.push({
      severity: "error",
      code: "missing_sprint_state",
      message: "No active Git metadata sprint-branch/*/state.json files were found.",
    })
    return null
  }
  if (input.json || !process.stdin.isTTY || !process.stdout.isTTY) {
    diagnostics.push({
      severity: "error",
      code: "sprint_selection_required",
      message:
        "No sprint could be inferred from a sprint branch or sprints/<name>. Pass the sprint name as an argument.",
    })
    return null
  }

  const selected = await autocomplete({
    message: "Select sprint",
    placeholder: "Type to filter sprints...",
    options: activeCandidates.map((candidate) => ({
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

  return activeCandidates.find((candidate) => candidate.sprint === selected) ?? null
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

  const statePath = await sprintStatePath(rootDir, sprint)
  try {
    const parsed = await readSprintStateFile(statePath)
    diagnostics.push(...parsed.diagnostics)
    return parsed.state ? candidateFromState(rootDir, statePath, parsed.state) : null
  } catch (error) {
    if (isMissingFileError(error)) {
      diagnostics.push({
        severity: "error",
        code: "missing_sprint_state",
        message: `Sprint state ${sprintStateDisplayPath(sprint)} does not exist.`,
      })
      return null
    }
    throw error
  }
}

async function readSprintCandidates(rootDir: string, includeParked = false) {
  const stateFiles = await findSprintStateFiles(rootDir)
  const candidates: SprintCandidate[] = []

  for (const statePath of stateFiles) {
    try {
      const parsed = await readSprintStateFile(statePath)
      if (parsed.state && (includeParked || parsed.state.visibility === "active")) {
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
    stateRelativePath: statePathForDisplay(rootDir, statePath),
    reviewBranch: state.branches.review,
    state,
  }
}

function statePathForDisplay(rootDir: string, statePath: string) {
  const parts = statePath.split(path.sep)
  const rootIndex = parts.lastIndexOf("sprint-branch")
  if (rootIndex !== -1) {
    return path.join(".git", ...parts.slice(rootIndex))
  }
  return path.relative(rootDir, statePath)
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
