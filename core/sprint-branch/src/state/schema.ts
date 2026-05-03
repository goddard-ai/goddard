import type {
  SprintActiveStash,
  SprintBranchState,
  SprintDiagnostic,
  SprintTaskState,
} from "../types"
import { getExpectedBranches, validateSprintName } from "./branches"

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

  const sprint = readString(record.sprint, "sprint", diagnostics)
  const baseBranch = readString(record.baseBranch, "baseBranch", diagnostics)
  const visibility = readVisibility(record.visibility, diagnostics)
  const tasks = parseTasks(record.tasks, diagnostics)
  const activeStashes = parseActiveStashes(record.activeStashes, diagnostics)
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

  if (!sprint || !baseBranch || !tasks || diagnostics.some(hasError)) {
    return {
      state: null,
      diagnostics,
    }
  }

  for (const diagnostic of validateSprintName(sprint)) {
    diagnostics.push(diagnostic)
  }

  if (diagnostics.some(hasError)) {
    return {
      state: null,
      diagnostics,
    }
  }

  const state: SprintBranchState = {
    sprint,
    baseBranch,
    visibility,
    branches: getExpectedBranches(sprint),
    tasks,
    activeStashes,
    conflict,
  }

  return {
    state,
    diagnostics,
  }
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
  const finishedUnreviewed =
    value.finishedUnreviewed === undefined
      ? []
      : readStringArray(value.finishedUnreviewed, "tasks.finishedUnreviewed", diagnostics)

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
