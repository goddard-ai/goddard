import * as fs from "node:fs/promises"
import path from "node:path"

import type { SprintBranchState, SprintTaskState } from "../types"

/** Resolves the next unassigned task file stem from sprint task queue order. */
export async function resolveNextPlannedTask(rootDir: string, state: SprintBranchState) {
  const tasks = await listTaskStems(rootDir, state.sprint)
  const assigned = new Set([
    ...state.tasks.approved,
    ...state.tasks.finishedUnreviewed,
    state.tasks.review,
    state.tasks.next,
  ])
  return tasks.find((task) => !assigned.has(task)) ?? null
}

/** Checks whether a requested sprint task file exists. */
export function taskFileExists(rootDir: string, sprint: string, task: string) {
  return pathExists(path.join(rootDir, "sprints", sprint, `${task}.md`))
}

/** Normalizes a task argument into a sprint-local task stem. */
export function normalizeTaskName(task: string) {
  const normalized = task.endsWith(".md") ? task.slice(0, -".md".length) : task
  if (normalized.includes("/") || normalized.includes("\\") || normalized.length === 0) {
    throw new Error("Task must be a task file stem inside the sprint folder.")
  }
  return normalized
}

/** Diagnostic emitted when a requested task skips ahead in queue order. */
export function nextTaskDiagnostic(task: string, plannedTask: string) {
  return {
    severity: "error" as const,
    code: "task_out_of_order",
    message: `${task} is not the next planned task; expected ${plannedTask}.`,
  }
}

/** Diagnostic emitted when no unassigned task remains for start. */
export function noPlannedTaskDiagnostic(task: string) {
  return {
    severity: "error" as const,
    code: "no_planned_task",
    message: `${task} is not available because no unassigned sprint tasks remain.`,
  }
}

/** Creates an empty task assignment record for new sprint state. */
export function emptyTasks(): SprintTaskState {
  return {
    review: null,
    next: null,
    approved: [],
    finishedUnreviewed: [],
  }
}

/** Deep-clones sprint state before planning a mutation. */
export function cloneState(state: SprintBranchState): SprintBranchState {
  return JSON.parse(JSON.stringify(state)) as SprintBranchState
}

/** Finds a feedback stash that belongs to the currently recorded next task. */
export function findMatchingStash(state: SprintBranchState) {
  return state.activeStashes.find(
    (stash) =>
      stash.sourceBranch === state.branches.next &&
      stash.task === state.tasks.next &&
      stash.reason === "feedback",
  )
}

async function listTaskStems(rootDir: string, sprint: string) {
  const sprintDir = path.join(rootDir, "sprints", sprint)
  const entries = await fs.readdir(sprintDir, { withFileTypes: true })
  return entries
    .filter((entry) => entry.isFile())
    .map((entry) => entry.name)
    .filter((name) => name.endsWith(".md"))
    .filter((name) => name !== "000-index.md" && name !== "001-handoff.md")
    .map((name) => name.slice(0, -".md".length))
    .sort()
}

async function pathExists(pathname: string) {
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

function isMissingFileError(error: unknown) {
  return (
    typeof error === "object" &&
    error !== null &&
    "code" in error &&
    (error as { code?: unknown }).code === "ENOENT"
  )
}
