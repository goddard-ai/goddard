import * as fs from "node:fs/promises"
import path from "node:path"

import { getCurrentBranch, resolveRepositoryRoot } from "./git"
import type {
  SprintActiveStash,
  SprintBranchNames,
  SprintBranchRole,
  SprintBranchState,
  SprintContext,
  SprintDiagnostic,
  SprintTaskState,
} from "./types"

export const sprintStateFileName = ".sprint-branch-state.json"
export const sprintIndexFileName = "000-index.md"

const branchPattern = /^sprint\/([^/]+)\/(review|approved|next)$/

/** Error raised when the current sprint cannot be inferred safely. */
export class SprintInferenceError extends Error {
  diagnostics: SprintDiagnostic[]

  constructor(message: string, diagnostics: SprintDiagnostic[] = []) {
    super(message)
    this.name = "SprintInferenceError"
    this.diagnostics = diagnostics
  }
}

/** Validates a sprint folder name before using it in a branch or path. */
export function validateSprintName(name: string) {
  const diagnostics: SprintDiagnostic[] = []

  if (name.length === 0) {
    diagnostics.push({
      severity: "error",
      code: "empty_sprint_name",
      message: "Sprint name cannot be empty.",
    })
  }
  if (name.includes("/") || name.includes("\\") || name === "." || name === "..") {
    diagnostics.push({
      severity: "error",
      code: "invalid_sprint_path_segment",
      message: "Sprint name must be a single sprints/<name> path segment.",
    })
  }
  if (/\s/.test(name)) {
    diagnostics.push({
      severity: "error",
      code: "invalid_sprint_whitespace",
      message: "Sprint name cannot contain whitespace.",
    })
  }

  return diagnostics
}

/** Extracts a sprint name and branch role from a sprint branch name. */
export function parseSprintBranchName(branch: string) {
  const match = branch.match(branchPattern)
  if (!match) {
    return null
  }

  return {
    sprint: match[1],
    role: match[2] as SprintBranchRole,
  }
}

/** Returns the canonical branch names for one sprint. */
export function getExpectedBranches(sprint: string): SprintBranchNames {
  return {
    review: `sprint/${sprint}/review`,
    approved: `sprint/${sprint}/approved`,
    next: `sprint/${sprint}/next`,
  }
}

/** Parses and validates the canonical sprint branch state JSON object. */
export function parseSprintState(value: unknown) {
  const diagnostics: SprintDiagnostic[] = []
  const record = isRecord(value) ? value : null

  if (!record) {
    return {
      state: null,
      diagnostics: [
        {
          severity: "error" as const,
          code: "invalid_state_root",
          message: "Sprint state must be a JSON object.",
        },
      ],
    }
  }

  if (record.schemaVersion !== 1) {
    diagnostics.push({
      severity: "error",
      code: "unsupported_schema_version",
      message: `Unsupported sprint state schema version: ${String(record.schemaVersion)}.`,
    })
  }

  const sprint = readString(record.sprint, "sprint", diagnostics)
  const baseBranch = readString(record.baseBranch, "baseBranch", diagnostics)
  const branches = parseBranches(record.branches, diagnostics)
  const tasks = parseTasks(record.tasks, diagnostics)
  const activeStashes = parseActiveStashes(record.activeStashes, diagnostics)
  const lock = "lock" in record ? record.lock : null
  const conflict =
    record.conflict === null || isRecord(record.conflict)
      ? (record.conflict as SprintBranchState["conflict"] | null)
      : null

  if (!("conflict" in record) || (record.conflict !== null && !isRecord(record.conflict))) {
    diagnostics.push({
      severity: "error",
      code: "invalid_conflict",
      message: "conflict must be null or an object.",
    })
  }

  if (!sprint || !baseBranch || !branches || !tasks || diagnostics.some(hasError)) {
    return {
      state: null,
      diagnostics,
    }
  }

  for (const diagnostic of validateSprintName(sprint)) {
    diagnostics.push(diagnostic)
  }

  const expectedBranches = getExpectedBranches(sprint)
  for (const role of Object.keys(expectedBranches) as SprintBranchRole[]) {
    if (branches[role] !== expectedBranches[role]) {
      diagnostics.push({
        severity: "error",
        code: "unexpected_branch_name",
        message: `${role} branch must be ${expectedBranches[role]}, but state records ${branches[role]}.`,
      })
    }
  }

  if (diagnostics.some(hasError)) {
    return {
      state: null,
      diagnostics,
    }
  }

  const state: SprintBranchState = {
    schemaVersion: 1,
    sprint,
    baseBranch,
    branches,
    tasks,
    activeStashes,
    lock,
    conflict,
  }

  return {
    state,
    diagnostics,
  }
}

/** Reads and validates one sprint branch state file. */
export async function readSprintStateFile(statePath: string) {
  const text = await fs.readFile(statePath, "utf-8")
  return parseSprintState(JSON.parse(text) as unknown)
}

/** Finds sprint branch state files immediately under sprints/<name>. */
export async function findSprintStateFiles(rootDir: string) {
  const sprintsDir = path.join(rootDir, "sprints")
  try {
    const entries = await fs.readdir(sprintsDir, { withFileTypes: true })
    const statePaths = entries
      .filter((entry) => entry.isDirectory())
      .map((entry) => path.join(sprintsDir, entry.name, sprintStateFileName))

    const existing = await Promise.all(
      statePaths.map(async (statePath) => ({
        statePath,
        exists: await asyncBooleanPathExists(statePath),
      })),
    )
    return existing.filter((entry) => entry.exists).map((entry) => entry.statePath)
  } catch (error) {
    if (isMissingFileError(error)) {
      return []
    }
    throw error
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

  const stateFiles = await resolveExistingStateFiles(rootDir)
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

/** Returns the canonical state path for the inferred sprint context. */
export function sprintStatePath(rootDir: string, sprint: string) {
  return path.join(rootDir, "sprints", sprint, sprintStateFileName)
}

/** Returns the canonical index mirror path for the inferred sprint context. */
export function sprintIndexPath(rootDir: string, sprint: string) {
  return path.join(rootDir, "sprints", sprint, sprintIndexFileName)
}

async function asyncBooleanPathExists(pathname: string) {
  try {
    await fs.access(pathname)
    return true
  } catch (error) {
    if (isMissingFileError(error)) {
      return false
    }
    throw error
  }
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

async function resolveExistingStateFiles(rootDir: string) {
  return findSprintStateFiles(rootDir)
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

function parseBranches(value: unknown, diagnostics: SprintDiagnostic[]) {
  if (!isRecord(value)) {
    diagnostics.push({
      severity: "error",
      code: "invalid_branches",
      message: "branches must be an object with review, approved, and next names.",
    })
    return null
  }

  const review = readString(value.review, "branches.review", diagnostics)
  const approved = readString(value.approved, "branches.approved", diagnostics)
  const next = readString(value.next, "branches.next", diagnostics)

  if (!review || !approved || !next) {
    return null
  }

  return { review, approved, next }
}

function parseTasks(value: unknown, diagnostics: SprintDiagnostic[]) {
  if (!isRecord(value)) {
    diagnostics.push({
      severity: "error",
      code: "invalid_tasks",
      message: "tasks must be an object.",
    })
    return null
  }

  const review = readOptionalString(value.review, "tasks.review", diagnostics)
  const next = readOptionalString(value.next, "tasks.next", diagnostics)
  const approved = readStringArray(value.approved, "tasks.approved", diagnostics)
  const finishedUnreviewed = readStringArray(
    value.finishedUnreviewed,
    "tasks.finishedUnreviewed",
    diagnostics,
  )

  if (!approved || !finishedUnreviewed || diagnostics.some(hasError)) {
    return null
  }

  return {
    review,
    next,
    approved,
    finishedUnreviewed,
  } satisfies SprintTaskState
}

function parseActiveStashes(value: unknown, diagnostics: SprintDiagnostic[]) {
  if (!Array.isArray(value)) {
    diagnostics.push({
      severity: "error",
      code: "invalid_active_stashes",
      message: "activeStashes must be an array.",
    })
    return []
  }

  const stashes: SprintActiveStash[] = []
  for (const [index, stash] of value.entries()) {
    if (!isRecord(stash)) {
      diagnostics.push({
        severity: "error",
        code: "invalid_active_stash",
        message: `activeStashes[${index}] must be an object.`,
      })
      continue
    }

    stashes.push({
      ref: typeof stash.ref === "string" ? stash.ref : undefined,
      sourceBranch: typeof stash.sourceBranch === "string" ? stash.sourceBranch : undefined,
      task: typeof stash.task === "string" || stash.task === null ? stash.task : undefined,
      reason: typeof stash.reason === "string" ? stash.reason : undefined,
      message: typeof stash.message === "string" ? stash.message : undefined,
    })
  }

  return stashes
}

function readString(value: unknown, field: string, diagnostics: SprintDiagnostic[]) {
  if (typeof value === "string" && value.length > 0) {
    return value
  }

  diagnostics.push({
    severity: "error",
    code: "invalid_string",
    message: `${field} must be a non-empty string.`,
  })
  return null
}

function readOptionalString(value: unknown, field: string, diagnostics: SprintDiagnostic[]) {
  if (value === null || value === undefined) {
    return null
  }
  if (typeof value === "string" && value.length > 0) {
    return value
  }

  diagnostics.push({
    severity: "error",
    code: "invalid_optional_string",
    message: `${field} must be null or a non-empty string.`,
  })
  return null
}

function readStringArray(value: unknown, field: string, diagnostics: SprintDiagnostic[]) {
  if (Array.isArray(value) && value.every((entry) => typeof entry === "string")) {
    return value
  }

  diagnostics.push({
    severity: "error",
    code: "invalid_string_array",
    message: `${field} must be an array of strings.`,
  })
  return null
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value)
}

function hasError(diagnostic: SprintDiagnostic) {
  return diagnostic.severity === "error"
}

function isMissingFileError(error: unknown) {
  return (
    typeof error === "object" &&
    error !== null &&
    "code" in error &&
    (error as { code?: unknown }).code === "ENOENT"
  )
}
