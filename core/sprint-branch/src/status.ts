import * as fs from "node:fs/promises"
import path from "node:path"

import { branchExists, getBranchHead, isAncestor } from "./git/refs"
import { resolveGitCommonPath } from "./git/repository"
import { getStashRefs } from "./git/stash"
import { getWorkingTreeStatus } from "./git/worktree"
import { inferSprintContext, type SprintInferenceInput } from "./state/inference"
import { readSprintStateFile } from "./state/io"
import type { SprintBranchRole, SprintBranchState, SprintStatusReport } from "./types"

const sprintRoles: SprintBranchRole[] = ["review", "approved", "next"]

/** Builds the read-only sprint branch status report used by status, diff, and doctor. */
export async function buildStatusReport(input: SprintInferenceInput) {
  const context = await inferSprintContext(input)
  const parsed = await readSprintStateFile(context.statePath)
  const diagnostics = [...parsed.diagnostics]

  if (!parsed.state) {
    return {
      context,
      report: null,
      diagnostics,
    }
  }

  const lockFilePath = await resolveGitCommonPath(
    context.rootDir,
    `sprint-branch/${context.sprint}.lock`,
  )

  const branches = {
    review: await inspectBranch(context.rootDir, parsed.state.branches.review),
    approved: await inspectBranch(context.rootDir, parsed.state.branches.approved),
    next: await inspectBranch(context.rootDir, parsed.state.branches.next),
  }
  const workingTree = await getWorkingTreeStatus(context.rootDir)
  const stashRefs = await getStashRefs(context.rootDir)
  const missingTaskFiles = await findMissingTaskFiles(context.rootDir, parsed.state)

  if (await pathExists(lockFilePath)) {
    diagnostics.push({
      severity: "error",
      code: "lock_file_exists",
      message: `Lock file ${path.join(".git", "sprint-branch", `${context.sprint}.lock`)} exists.`,
      suggestion: "Confirm no sprint-branch command is running before removing the lock.",
    })
  }

  if (!branches.approved.exists) {
    diagnostics.push({
      severity: "error",
      code: "approved_branch_missing",
      message: `Approved branch ${parsed.state.branches.approved} does not exist.`,
      suggestion: `sprint-branch init --sprint ${parsed.state.sprint} --base ${parsed.state.baseBranch}`,
    })
  }
  if (!branches.review.exists) {
    diagnostics.push({
      severity: "error",
      code: "review_branch_missing",
      message: `Review branch ${parsed.state.branches.review} does not exist.`,
      suggestion: `sprint-branch init --sprint ${parsed.state.sprint} --base ${parsed.state.baseBranch}`,
    })
  }
  if (parsed.state.tasks.next && !branches.next.exists) {
    diagnostics.push({
      severity: "error",
      code: "next_branch_missing",
      message: `Next branch ${parsed.state.branches.next} is missing while task ${parsed.state.tasks.next} is recorded on next.`,
      suggestion: "Run sprint-branch doctor before resuming work-ahead changes.",
    })
  }
  if (context.currentBranch === parsed.state.branches.approved && !workingTree.clean) {
    diagnostics.push({
      severity: "error",
      code: "dirty_approved_worktree",
      message: `${parsed.state.branches.approved} has uncommitted changes; approved must contain only human-approved work.`,
      suggestion: "Move implementation work to review or next before continuing.",
    })
  }
  if (context.currentBranch === parsed.state.branches.next && !parsed.state.tasks.next) {
    diagnostics.push({
      severity: "warning",
      code: "next_branch_without_task",
      message: `${parsed.state.branches.next} is checked out but no next task is recorded.`,
      suggestion: "Run sprint-branch doctor before using the next branch.",
    })
  }

  const reviewDescendsFromApproved =
    branches.approved.exists && branches.review.exists
      ? await isAncestor(
          context.rootDir,
          parsed.state.branches.approved,
          parsed.state.branches.review,
        )
      : null
  const nextDescendsFromReview =
    branches.next.exists && branches.review.exists
      ? await isAncestor(context.rootDir, parsed.state.branches.review, parsed.state.branches.next)
      : null
  if (reviewDescendsFromApproved === false) {
    diagnostics.push({
      severity: "error",
      code: "review_not_based_on_approved",
      message: `${parsed.state.branches.review} does not descend from ${parsed.state.branches.approved}.`,
      suggestion: "Manual recovery is required before sprint-branch can safely continue.",
    })
  }
  if (nextDescendsFromReview === false) {
    diagnostics.push({
      severity: "error",
      code: "next_not_based_on_review",
      message: `${parsed.state.branches.next} does not descend from ${parsed.state.branches.review}.`,
      suggestion: "Run sprint-branch resume --dry-run before continuing work-ahead changes.",
    })
  }

  for (const stash of parsed.state.activeStashes) {
    if (stash.ref && !stashRefs.has(stash.ref)) {
      diagnostics.push({
        severity: "error",
        code: "recorded_stash_missing",
        message: `Recorded stash ${stash.ref} is not present in git stash list.`,
        suggestion: "Run sprint-branch doctor before applying or dropping any stash manually.",
      })
    }
  }
  for (const task of missingTaskFiles) {
    diagnostics.push({
      severity: "warning",
      code: "task_file_missing",
      message: `Recorded task ${task} does not have a sprints/${parsed.state.sprint}/${task}.md file.`,
      suggestion: "Confirm the task mapping before mutating sprint branches.",
    })
  }

  const blockedReasons = resolveBlockedReasons(
    parsed.state,
    context.currentBranch,
    workingTree.clean,
  )
  const report: SprintStatusReport = {
    ok: !diagnostics.some((diagnostic) => diagnostic.severity === "error"),
    rootDir: context.rootDir,
    sprint: parsed.state.sprint,
    statePath: context.statePath,
    stateRelativePath: context.stateRelativePath,
    currentBranch: context.currentBranch,
    inferredFrom: context.inferredFrom,
    state: parsed.state,
    branches,
    ancestry: {
      reviewDescendsFromApproved,
      nextDescendsFromReview,
    },
    workingTree,
    blocked: {
      review: Boolean(parsed.state.tasks.review),
      conflict: parsed.state.conflict !== null,
      feedback:
        context.currentBranch === parsed.state.branches.next &&
        !workingTree.clean &&
        Boolean(parsed.state.tasks.next),
      reasons: blockedReasons,
      nextSafeCommand: resolveNextSafeCommand(
        parsed.state,
        context.currentBranch,
        workingTree.clean,
      ),
    },
    diagnostics,
  }

  return {
    context,
    report,
    diagnostics,
  }
}

/** Formats a status report for human-oriented terminal output. */
export function formatStatusReport(report: SprintStatusReport) {
  const lines = [
    `Sprint: ${report.sprint}`,
    `State: ${report.stateRelativePath}`,
    `Inferred from: ${report.inferredFrom}`,
    `Current branch: ${report.currentBranch ?? "detached HEAD"}`,
    "",
    "Branches:",
  ]

  for (const role of sprintRoles) {
    const branch = report.branches[role]
    lines.push(
      `  ${role}: ${branch.name} ${branch.exists ? `(${branch.head?.slice(0, 12)})` : "(missing)"}`,
    )
  }

  lines.push(
    "",
    "Ancestry:",
    `  review descends from approved: ${formatNullableBoolean(report.ancestry.reviewDescendsFromApproved)}`,
    `  next descends from review: ${formatNullableBoolean(report.ancestry.nextDescendsFromReview)}`,
    "",
    "Tasks:",
    `  review: ${report.state.tasks.review ?? "none"}`,
    `  next: ${report.state.tasks.next ?? "none"}`,
    `  approved: ${report.state.tasks.approved.length ? report.state.tasks.approved.join(", ") : "none"}`,
    "",
    `Working tree: ${report.workingTree.clean ? "clean" : "dirty"}`,
  )

  if (!report.workingTree.clean) {
    lines.push(...report.workingTree.entries.map((entry) => `  ${entry}`))
  }

  lines.push(
    "",
    `Blocked: ${report.blocked.reasons.length ? report.blocked.reasons.join("; ") : "no"}`,
  )

  if (report.blocked.nextSafeCommand) {
    lines.push(`Next safe command: ${report.blocked.nextSafeCommand}`)
  }

  if (report.diagnostics.length > 0) {
    lines.push("", "Diagnostics:")
    for (const diagnostic of report.diagnostics) {
      lines.push(`  [${diagnostic.severity}] ${diagnostic.code}: ${diagnostic.message}`)
      if (diagnostic.suggestion) {
        lines.push(`    suggestion: ${diagnostic.suggestion}`)
      }
    }
  }

  return lines.join("\n")
}

async function inspectBranch(rootDir: string, name: string) {
  const exists = await branchExists(rootDir, name)
  return {
    name,
    exists,
    head: exists ? await getBranchHead(rootDir, name) : null,
  }
}

async function findMissingTaskFiles(rootDir: string, state: SprintBranchState) {
  const tasks = new Set(
    [state.tasks.review, state.tasks.next, ...state.tasks.approved].filter(
      (task): task is string => typeof task === "string" && task.length > 0,
    ),
  )
  const missing: string[] = []

  for (const task of tasks) {
    try {
      await fs.access(path.join(rootDir, "sprints", state.sprint, `${task}.md`))
    } catch (error) {
      if (isMissingFileError(error)) {
        missing.push(task)
        continue
      }
      throw error
    }
  }

  return missing
}

function resolveBlockedReasons(
  state: SprintBranchState,
  currentBranch: string | null,
  clean: boolean,
) {
  const reasons: string[] = []

  if (state.conflict) {
    reasons.push("conflict recorded")
  }
  if (state.tasks.review) {
    reasons.push(`review branch carries ${state.tasks.review}`)
  }
  if (currentBranch === state.branches.next && !clean && state.tasks.next) {
    reasons.push("dirty next-branch work should be interrupted with feedback before review changes")
  }

  return reasons
}

function resolveNextSafeCommand(
  state: SprintBranchState,
  currentBranch: string | null,
  clean: boolean,
) {
  if (state.conflict) {
    return "sprint-branch doctor"
  }
  if (currentBranch === state.branches.next && !clean && state.tasks.next) {
    return "sprint-branch feedback --dry-run"
  }
  if (state.tasks.review) {
    return "sprint-branch approve --dry-run after human approval"
  }
  if (state.tasks.next) {
    return "sprint-branch resume --dry-run"
  }
  return "sprint-branch start --task <task-file> --dry-run"
}

function formatNullableBoolean(value: boolean | null) {
  if (value === null) {
    return "not applicable"
  }
  return value ? "yes" : "no"
}

function isMissingFileError(error: unknown) {
  return (
    typeof error === "object" &&
    error !== null &&
    "code" in error &&
    (error as { code?: unknown }).code === "ENOENT"
  )
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
