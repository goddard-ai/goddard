import { isObject } from "radashi"

import { hasDiagnosticErrors } from "../diagnostics"
import type {
  SprintActiveStash,
  SprintBranchState,
  SprintDiagnostic,
  SprintTaskState,
} from "../types"
import { getExpectedBranches, validateSprintName } from "./branches"

/** Defaults used when reading state files written before newer fields existed. */
type SprintStateParseOptions = {
  defaultSprintWorktreeRoot?: string
}

/** Parses and validates the canonical sprint branch state JSON object. */
export function parseSprintState(value: unknown, options: SprintStateParseOptions = {}) {
  const diagnostics: SprintDiagnostic[] = []
  const record = isObject(value) ? (value as Record<string, unknown>) : null

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

  const sprint = readString(record.sprint, "sprint", diagnostics)
  const baseBranch = readString(record.baseBranch, "baseBranch", diagnostics)
  const sprintWorktreeRoot = readString(
    record.sprintWorktreeRoot ?? options.defaultSprintWorktreeRoot,
    "sprintWorktreeRoot",
    diagnostics,
  )
  const visibility = readVisibility(record.visibility, diagnostics)
  const lastActedAt = readOptionalTimestamp(record.lastActedAt, diagnostics)
  const tasks = parseTasks(record.tasks, diagnostics)
  const activeStashes = parseActiveStashes(record.activeStashes, diagnostics)
  const ignoredNextBranchAtFinalize = parseIgnoredNextBranchAtFinalize(
    record.ignoredNextBranchAtFinalize,
    diagnostics,
  )
  const conflict =
    record.conflict === null || isObject(record.conflict)
      ? (record.conflict as SprintBranchState["conflict"] | null)
      : null

  if (!("conflict" in record) || (record.conflict !== null && !isObject(record.conflict))) {
    diagnostics.push({
      severity: "error",
      code: "invalid_conflict",
      message: "conflict must be null or an object.",
    })
  }

  if (!sprint || !baseBranch || !sprintWorktreeRoot || !tasks || hasDiagnosticErrors(diagnostics)) {
    return {
      state: null,
      diagnostics,
    }
  }

  for (const diagnostic of validateSprintName(sprint)) {
    diagnostics.push(diagnostic)
  }

  if (hasDiagnosticErrors(diagnostics)) {
    return {
      state: null,
      diagnostics,
    }
  }

  const state: SprintBranchState = {
    sprint,
    baseBranch,
    sprintWorktreeRoot,
    visibility,
    lastActedAt,
    branches: getExpectedBranches(sprint),
    tasks,
    activeStashes,
    ignoredNextBranchAtFinalize,
    conflict,
  }

  return {
    state,
    diagnostics,
  }
}

function parseTasks(value: unknown, diagnostics: SprintDiagnostic[]) {
  if (!isObject(value)) {
    diagnostics.push({
      severity: "error",
      code: "invalid_tasks",
      message: "tasks must be an object.",
    })
    return null
  }

  const record = value as Record<string, unknown>
  const review = readOptionalString(record.review, "tasks.review", diagnostics)
  const next = readOptionalString(record.next, "tasks.next", diagnostics)
  const approved = readStringArray(record.approved, "tasks.approved", diagnostics)
  const finishedUnreviewed =
    record.finishedUnreviewed === undefined
      ? []
      : readStringArray(record.finishedUnreviewed, "tasks.finishedUnreviewed", diagnostics)

  if (!approved || !finishedUnreviewed || hasDiagnosticErrors(diagnostics)) {
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
    if (!isObject(stash)) {
      diagnostics.push({
        severity: "error",
        code: "invalid_active_stash",
        message: `activeStashes[${index}] must be an object.`,
      })
      continue
    }

    const record = stash as Record<string, unknown>
    stashes.push({
      ref: typeof record.ref === "string" ? record.ref : undefined,
      sourceBranch: typeof record.sourceBranch === "string" ? record.sourceBranch : undefined,
      task: typeof record.task === "string" || record.task === null ? record.task : undefined,
      reason: typeof record.reason === "string" ? record.reason : undefined,
      message: typeof record.message === "string" ? record.message : undefined,
    })
  }

  return stashes
}

function parseIgnoredNextBranchAtFinalize(value: unknown, diagnostics: SprintDiagnostic[]) {
  if (value === null || value === undefined) {
    return null
  }
  if (!isObject(value)) {
    diagnostics.push({
      severity: "error",
      code: "invalid_ignored_next_branch_at_finalize",
      message: "ignoredNextBranchAtFinalize must be null or an object.",
    })
    return null
  }

  const record = value as Record<string, unknown>
  const reviewCommit = readString(
    record.reviewCommit,
    "ignoredNextBranchAtFinalize.reviewCommit",
    diagnostics,
  )
  const nextCommit = readString(
    record.nextCommit,
    "ignoredNextBranchAtFinalize.nextCommit",
    diagnostics,
  )

  return reviewCommit && nextCommit
    ? {
        reviewCommit,
        nextCommit,
      }
    : null
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

function readVisibility(value: unknown, diagnostics: SprintDiagnostic[]) {
  if (value === undefined) {
    return "active"
  }
  if (value === "active" || value === "parked") {
    return value
  }

  diagnostics.push({
    severity: "error",
    code: "invalid_visibility",
    message: "visibility must be active or parked.",
  })
  return "active"
}

function readOptionalTimestamp(value: unknown, diagnostics: SprintDiagnostic[]) {
  if (value === null || value === undefined) {
    return null
  }
  if (typeof value === "string" && value.length > 0 && !Number.isNaN(Date.parse(value))) {
    return value
  }

  diagnostics.push({
    severity: "warning",
    code: "invalid_last_acted_at",
    message: "lastActedAt must be null or a valid timestamp string.",
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
