import * as fs from "node:fs/promises"
import path from "node:path"

import { branchExists, getStashRefs, resolveGitPath } from "./git"
import { buildStatusReport } from "./status"
import type { SprintBranchState, SprintDiagnostic, SprintStatusReport } from "./types"

const generatedBlockStart = "<!-- sprint-branch-state:start -->"
const generatedBlockEnd = "<!-- sprint-branch-state:end -->"
const taskStemPattern = /^\d{3}-[a-z0-9][a-z0-9-]*$/

/** Builds the deeper consistency report for sprint-branch doctor. */
export async function buildDoctorReport(input: { cwd: string; sprint?: string }) {
  const { context, report, diagnostics } = await buildStatusReport(input)
  if (!report) {
    return { context, report, diagnostics }
  }

  const doctorDiagnostics = dedupeDiagnostics([
    ...report.diagnostics,
    ...(await runDoctorChecks(report)),
  ])
  const doctorReport: SprintStatusReport = {
    ...report,
    ok: !doctorDiagnostics.some((diagnostic) => diagnostic.severity === "error"),
    diagnostics: doctorDiagnostics,
  }

  return {
    context,
    report: doctorReport,
    diagnostics: doctorDiagnostics,
  }
}

/** Formats doctor diagnostics with a concise recovery suggestion. */
export function formatDoctorReport(report: SprintStatusReport) {
  if (report.diagnostics.length === 0) {
    return [
      `No sprint branch issues found for ${report.sprint}.`,
      `Next safe command: ${report.blocked.nextSafeCommand ?? "sprint-branch status"}`,
    ].join("\n")
  }

  const lines = [`Sprint branch issues for ${report.sprint}:`]
  for (const diagnostic of report.diagnostics) {
    lines.push(`  [${diagnostic.severity}] ${diagnostic.code}: ${diagnostic.message}`)
    if (diagnostic.suggestion) {
      lines.push(`    suggestion: ${diagnostic.suggestion}`)
    }
  }

  lines.push(`Next safe command: ${report.blocked.nextSafeCommand ?? "manual recovery required"}`)
  return lines.join("\n")
}

async function runDoctorChecks(report: SprintStatusReport) {
  const diagnostics: SprintDiagnostic[] = []

  diagnostics.push(...(await checkBaseBranches(report)))
  diagnostics.push(...checkBranchDrift(report))
  diagnostics.push(...checkTaskAssignments(report))
  diagnostics.push(...(await checkTaskQueue(report)))
  diagnostics.push(...(await checkStashes(report)))
  diagnostics.push(...(await checkGitOperationState(report)))
  diagnostics.push(...(await checkGeneratedIndexBlock(report)))
  diagnostics.push(...(await checkHandoff(report)))
  diagnostics.push(...checkCurrentBranch(report))

  return diagnostics
}

async function checkBaseBranches(report: SprintStatusReport) {
  const diagnostics: SprintDiagnostic[] = []
  const { state } = report

  if (!(await branchExists(report.rootDir, state.baseBranch))) {
    diagnostics.push({
      severity: "error",
      code: "base_branch_missing",
      message: `Base branch ${state.baseBranch} does not exist.`,
      suggestion:
        "Use sprint-branch finalize --override-base <branch> only after confirming the intended base.",
    })
  }
  if (Object.values(state.branches).includes(state.baseBranch)) {
    diagnostics.push({
      severity: "error",
      code: "base_branch_is_sprint_branch",
      message: `Base branch ${state.baseBranch} must not be one of the sprint branches.`,
    })
  }
  if (await branchExists(report.rootDir, `sprint/${state.sprint}`)) {
    diagnostics.push({
      severity: "error",
      code: "bare_sprint_namespace_branch_exists",
      message: `Bare branch sprint/${state.sprint} exists; sprint/<name> is reserved as a namespace.`,
    })
  }

  return diagnostics
}

function checkBranchDrift(report: SprintStatusReport) {
  const diagnostics: SprintDiagnostic[] = []
  const { state } = report
  const approvedHead = report.branches.approved.head
  const reviewHead = report.branches.review.head
  const nextHead = report.branches.next.head

  if (!state.tasks.review && approvedHead && reviewHead && approvedHead !== reviewHead) {
    diagnostics.push({
      severity: "error",
      code: "review_branch_has_unrecorded_work",
      message: `${state.branches.review} differs from approved while no review task is recorded.`,
      suggestion:
        "Run sprint-branch start or recover the task mapping before approving/finalizing.",
    })
  }
  if (state.tasks.review && approvedHead && reviewHead && approvedHead === reviewHead) {
    diagnostics.push({
      severity: "warning",
      code: "review_task_has_no_diff",
      message: `Review task ${state.tasks.review} is recorded, but review and approved point at the same commit.`,
    })
  }
  if (!state.tasks.next && nextHead && reviewHead && nextHead !== reviewHead) {
    diagnostics.push({
      severity: "error",
      code: "next_branch_has_unrecorded_work",
      message: `${state.branches.next} differs from review while no next task is recorded.`,
      suggestion: "Recover or discard the unrecorded next branch work before continuing.",
    })
  }
  if (state.tasks.next && nextHead && reviewHead && nextHead === reviewHead) {
    diagnostics.push({
      severity: "warning",
      code: "next_task_has_no_diff",
      message: `Next task ${state.tasks.next} is recorded, but next and review point at the same commit.`,
    })
  }
  if (state.tasks.next && !state.tasks.review) {
    diagnostics.push({
      severity: "error",
      code: "next_task_without_review_task",
      message: `Next task ${state.tasks.next} is recorded without a current review task.`,
    })
  }
  if (state.tasks.finishedUnreviewed.length > 0) {
    diagnostics.push({
      severity: "warning",
      code: "finished_unreviewed_tasks_recorded",
      message: `Finished unreviewed tasks remain: ${state.tasks.finishedUnreviewed.join(", ")}.`,
    })
  }

  return diagnostics
}

function checkTaskAssignments(report: SprintStatusReport) {
  const diagnostics: SprintDiagnostic[] = []
  const seen = new Map<string, string[]>()

  for (const [location, task] of taskAssignments(report.state)) {
    if (!task) {
      continue
    }

    const locations = seen.get(task) ?? []
    locations.push(location)
    seen.set(task, locations)
  }

  for (const [task, locations] of seen) {
    if (locations.length > 1) {
      diagnostics.push({
        severity: "error",
        code: "task_assigned_multiple_roles",
        message: `Task ${task} is recorded in multiple places: ${locations.join(", ")}.`,
      })
    }
  }

  return diagnostics
}

async function checkTaskQueue(report: SprintStatusReport) {
  const diagnostics: SprintDiagnostic[] = []
  const taskFiles = await listTaskFiles(report.rootDir, report.state.sprint)
  const taskStems = taskFiles.map((task) => task.stem)
  const approved = new Set(report.state.tasks.approved)
  const finishedUnreviewed = new Set(report.state.tasks.finishedUnreviewed)

  for (const task of taskFiles) {
    if (!taskStemPattern.test(task.stem)) {
      diagnostics.push({
        severity: "warning",
        code: "task_file_stem_unusual",
        message: `Task file ${task.relativePath} does not match the expected 000-task-name pattern.`,
      })
    }
  }

  for (const [prefix, tasks] of groupByPrefix(taskFiles)) {
    if (tasks.length > 1) {
      diagnostics.push({
        severity: "error",
        code: "duplicate_task_file_prefix",
        message: `Task prefix ${prefix} is used by multiple task files: ${tasks
          .map((task) => task.stem)
          .join(", ")}.`,
      })
    }
  }

  if (report.state.tasks.review && taskStems.includes(report.state.tasks.review)) {
    const expectedReview = taskStems.find((task) => !approved.has(task))
    if (expectedReview && expectedReview !== report.state.tasks.review) {
      diagnostics.push({
        severity: "error",
        code: "review_task_out_of_order",
        message: `Review task ${report.state.tasks.review} is not the oldest unapproved task; expected ${expectedReview}.`,
      })
    }
  }

  if (report.state.tasks.next && taskStems.includes(report.state.tasks.next)) {
    const expectedNext = taskStems.find(
      (task) =>
        !approved.has(task) && !finishedUnreviewed.has(task) && task !== report.state.tasks.review,
    )
    if (expectedNext && expectedNext !== report.state.tasks.next) {
      diagnostics.push({
        severity: "error",
        code: "next_task_out_of_order",
        message: `Next task ${report.state.tasks.next} is not the next unassigned task; expected ${expectedNext}.`,
      })
    }
  }

  return diagnostics
}

async function checkStashes(report: SprintStatusReport) {
  const diagnostics: SprintDiagnostic[] = []
  const stashRefs = await getStashRefs(report.rootDir)
  const recordedRefs = new Set<string>()

  for (const stash of report.state.activeStashes) {
    if (!stash.ref) {
      diagnostics.push({
        severity: "error",
        code: "recorded_stash_missing_ref",
        message: "An active stash record is missing its stash ref.",
      })
      continue
    }

    recordedRefs.add(stash.ref)
    const message = stashRefs.get(stash.ref)
    if (message && stash.message && message !== stash.message) {
      diagnostics.push({
        severity: "warning",
        code: "recorded_stash_message_mismatch",
        message: `Recorded stash ${stash.ref} message does not match Git's stash message.`,
      })
    }
    if (stash.reason === "feedback" && stash.sourceBranch !== report.state.branches.next) {
      diagnostics.push({
        severity: "error",
        code: "feedback_stash_source_mismatch",
        message: `Feedback stash ${stash.ref} was recorded from ${stash.sourceBranch ?? "unknown"}, not ${report.state.branches.next}.`,
      })
    }
    if (stash.reason === "feedback" && stash.task !== report.state.tasks.next) {
      diagnostics.push({
        severity: "warning",
        code: "feedback_stash_task_mismatch",
        message: `Feedback stash ${stash.ref} is for ${stash.task ?? "unknown"}, but next is ${report.state.tasks.next ?? "none"}.`,
      })
    }
  }

  for (const [ref, message] of stashRefs) {
    if (message.includes(`sprint-branch:${report.state.sprint}:`) && !recordedRefs.has(ref)) {
      diagnostics.push({
        severity: "warning",
        code: "unrecorded_sprint_stash",
        message: `Git stash ${ref} looks like a sprint-branch stash but is not recorded in state.`,
      })
    }
  }

  return diagnostics
}

async function checkGitOperationState(report: SprintStatusReport) {
  const diagnostics: SprintDiagnostic[] = []
  const operations = await getGitOperations(report.rootDir)

  if (operations.length > 0 && !report.state.conflict) {
    diagnostics.push({
      severity: "error",
      code: "git_operation_without_conflict_state",
      message: `Git has an in-progress ${operations.map((operation) => operation.name).join(", ")} operation, but sprint state has no conflict record.`,
      suggestion: "Resolve or abort the Git operation before running sprint-branch transitions.",
    })
  }
  if (operations.length === 0 && report.state.conflict) {
    diagnostics.push({
      severity: "error",
      code: "conflict_state_without_git_operation",
      message: `Sprint state records a ${report.state.conflict.command ?? "unknown"} conflict, but Git has no matching operation in progress.`,
    })
  }
  if (report.state.conflict?.branch) {
    if (!(await branchExists(report.rootDir, report.state.conflict.branch))) {
      diagnostics.push({
        severity: "error",
        code: "conflict_branch_missing",
        message: `Conflict branch ${report.state.conflict.branch} no longer exists.`,
      })
    }
    if (report.currentBranch && report.currentBranch !== report.state.conflict.branch) {
      diagnostics.push({
        severity: "warning",
        code: "current_branch_not_conflict_branch",
        message: `Conflict is recorded on ${report.state.conflict.branch}, but current branch is ${report.currentBranch}.`,
      })
    }
  }

  return diagnostics
}

async function checkGeneratedIndexBlock(report: SprintStatusReport) {
  const diagnostics: SprintDiagnostic[] = []
  let text = ""

  try {
    text = await fs.readFile(report.index.path, "utf-8")
  } catch (error) {
    if (isMissingFileError(error)) {
      return diagnostics
    }
    throw error
  }

  const start = text.indexOf(generatedBlockStart)
  const end = text.indexOf(generatedBlockEnd)
  if (start === -1 && end === -1) {
    return diagnostics
  }
  if (start === -1 || end === -1 || end < start) {
    diagnostics.push({
      severity: "warning",
      code: "index_generated_block_malformed",
      message: `${report.index.relativePath} has an incomplete generated sprint-branch state block.`,
    })
    return diagnostics
  }

  const values = parseGeneratedIndexValues(text.slice(start, end))
  const expected = new Map([
    ["Sprint", report.state.sprint],
    ["Base branch", report.state.baseBranch],
    ["Review branch", report.state.branches.review],
    ["Approved branch", report.state.branches.approved],
    ["Next branch", report.state.branches.next],
    ["Review task", report.state.tasks.review ?? "none"],
    ["Next task", report.state.tasks.next ?? "none"],
    [
      "Approved tasks",
      report.state.tasks.approved.length ? report.state.tasks.approved.join(", ") : "none",
    ],
    [
      "Finished unreviewed",
      report.state.tasks.finishedUnreviewed.length
        ? report.state.tasks.finishedUnreviewed.join(", ")
        : "none",
    ],
  ])

  for (const [key, expectedValue] of expected) {
    const actual = values.get(key)
    if (actual !== undefined && actual !== expectedValue) {
      diagnostics.push({
        severity: "warning",
        code: "index_generated_block_value_mismatch",
        message: `${report.index.relativePath} records "${key}: ${actual}", but JSON state has "${expectedValue}".`,
        suggestion:
          "Run a mutating sprint-branch command only after confirming the JSON state is correct.",
      })
    }
  }

  return diagnostics
}

async function checkHandoff(report: SprintStatusReport) {
  const handoffPath = path.join(report.rootDir, "sprints", report.state.sprint, "001-handoff.md")
  if (await pathExists(handoffPath)) {
    return []
  }

  return [
    {
      severity: "warning" as const,
      code: "handoff_missing",
      message: `Handoff file ${path.relative(report.rootDir, handoffPath)} does not exist.`,
    },
  ]
}

function checkCurrentBranch(report: SprintStatusReport) {
  if (!report.currentBranch) {
    return [
      {
        severity: "warning" as const,
        code: "detached_head",
        message: "Repository is on detached HEAD while inspecting sprint branch state.",
      },
    ]
  }
  if (
    report.currentBranch.startsWith(`sprint/${report.state.sprint}/`) &&
    !Object.values(report.state.branches).includes(report.currentBranch)
  ) {
    return [
      {
        severity: "error" as const,
        code: "current_unrecorded_sprint_branch",
        message: `Current branch ${report.currentBranch} is inside the sprint namespace but is not recorded in state.`,
      },
    ]
  }
  if (report.currentBranch === report.state.branches.approved) {
    return [
      {
        severity: "warning" as const,
        code: "current_branch_is_approved",
        message: "Current branch is approved; implementation work should happen on review or next.",
      },
    ]
  }

  return []
}

function taskAssignments(state: SprintBranchState): Array<[string, string | null]> {
  return [
    ["tasks.review", state.tasks.review],
    ["tasks.next", state.tasks.next],
    ...state.tasks.approved.map((task, index): [string, string] => [
      `tasks.approved[${index}]`,
      task,
    ]),
    ...state.tasks.finishedUnreviewed.map((task, index): [string, string] => [
      `tasks.finishedUnreviewed[${index}]`,
      task,
    ]),
  ]
}

async function listTaskFiles(rootDir: string, sprint: string) {
  const sprintDir = path.join(rootDir, "sprints", sprint)
  try {
    const entries = await fs.readdir(sprintDir, { withFileTypes: true })
    return entries
      .filter((entry) => entry.isFile())
      .map((entry) => entry.name)
      .filter((name) => name.endsWith(".md"))
      .filter((name) => name !== "000-index.md" && name !== "001-handoff.md")
      .map((name) => ({
        stem: name.slice(0, -".md".length),
        relativePath: path.join("sprints", sprint, name),
      }))
      .sort((left, right) => left.stem.localeCompare(right.stem))
  } catch (error) {
    if (isMissingFileError(error)) {
      return []
    }
    throw error
  }
}

function groupByPrefix(tasks: Array<{ stem: string }>) {
  const prefixes = new Map<string, Array<{ stem: string }>>()
  for (const task of tasks) {
    const match = task.stem.match(/^(\d{3})-/)
    if (!match) {
      continue
    }

    prefixes.set(match[1], [...(prefixes.get(match[1]) ?? []), task])
  }
  return [...prefixes.entries()]
}

async function getGitOperations(rootDir: string) {
  const operationPaths = [
    ["rebase", "rebase-merge"],
    ["rebase", "rebase-apply"],
    ["merge", "MERGE_HEAD"],
    ["cherry-pick", "CHERRY_PICK_HEAD"],
    ["revert", "REVERT_HEAD"],
    ["bisect", "BISECT_LOG"],
  ]
  const operations: Array<{ name: string; path: string }> = []

  for (const [name, gitPath] of operationPaths) {
    const resolvedPath = await resolveGitPath(rootDir, gitPath)
    if (await pathExists(resolvedPath)) {
      operations.push({ name, path: resolvedPath })
    }
  }

  return operations
}

function parseGeneratedIndexValues(block: string) {
  const values = new Map<string, string>()
  for (const line of block.split("\n")) {
    const match = line.match(/^- ([^:]+): (.*)$/)
    if (match) {
      values.set(match[1], match[2])
    }
  }
  return values
}

function dedupeDiagnostics(diagnostics: SprintDiagnostic[]) {
  const seen = new Set<string>()
  const unique: SprintDiagnostic[] = []
  for (const diagnostic of diagnostics) {
    const key = `${diagnostic.severity}:${diagnostic.code}:${diagnostic.message}`
    if (seen.has(key)) {
      continue
    }
    seen.add(key)
    unique.push(diagnostic)
  }
  return unique
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
