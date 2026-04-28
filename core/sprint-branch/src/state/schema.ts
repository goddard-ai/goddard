import type {
  SprintActiveStash,
  SprintBranchRole,
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
  const lock = parseLock(record.lock, diagnostics)
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

function parseLock(value: unknown, diagnostics: SprintDiagnostic[]) {
  if (value === null || value === undefined) {
    return null
  }
  if (!isRecord(value)) {
    diagnostics.push({
      severity: "error",
      code: "invalid_lock",
      message: "lock must be null or an object.",
    })
    return null
  }

  const command = readString(value.command, "lock.command", diagnostics)
  const createdAt = readString(value.createdAt, "lock.createdAt", diagnostics)
  const pid = typeof value.pid === "number" ? value.pid : null

  if (pid === null) {
    diagnostics.push({
      severity: "error",
      code: "invalid_lock_pid",
      message: "lock.pid must be a number.",
    })
  }
  if (!command || !createdAt || pid === null) {
    return null
  }

  return { command, createdAt, pid }
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
