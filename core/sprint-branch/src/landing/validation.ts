import { getBranchHead, isAncestor } from "../git/refs"
import { getCurrentBranch } from "../git/repository"
import { getWorkingTreeStatus } from "../git/worktree"
import { parseSprintBranchName } from "../state/branches"
import type { SprintBranchState, SprintDiagnostic } from "../types"
import type { CleanupInput, HumanCommandInput, LandInput } from "./types"

/** Adds diagnostics for the finalized-state requirements specific to land. */
export async function pushLandingDiagnostics(
  rootDir: string,
  input: LandInput,
  state: SprintBranchState | null,
  reviewCommit: string | null,
  targetCommit: string | null,
  diagnostics: SprintDiagnostic[],
) {
  await pushSharedFinalizedDiagnostics(
    rootDir,
    input,
    state,
    reviewCommit,
    targetCommit,
    diagnostics,
  )
  if (
    state &&
    reviewCommit &&
    targetCommit &&
    !(await isAncestor(rootDir, input.target, state.branches.review))
  ) {
    diagnostics.push({
      severity: "error",
      code: "target_not_ancestor_of_review",
      message: `${input.target} cannot fast-forward to ${state.branches.review}.`,
      suggestion: `Run sprint-branch finalize --override-base ${input.target} before landing.`,
    })
  }
}

/** Adds diagnostics for deleting sprint refs and associated worktrees after landing. */
export async function pushCleanupDiagnostics(
  rootDir: string,
  input: CleanupInput,
  state: SprintBranchState | null,
  reviewCommit: string | null,
  targetCommit: string | null,
  branchesToDelete: string[],
  diagnostics: SprintDiagnostic[],
) {
  await pushSharedFinalizedDiagnostics(
    rootDir,
    input,
    state,
    reviewCommit,
    targetCommit,
    diagnostics,
  )
  if (
    state &&
    reviewCommit &&
    targetCommit &&
    !(await isAncestor(rootDir, state.branches.review, input.target))
  ) {
    diagnostics.push({
      severity: "error",
      code: "target_missing_review",
      message: `${input.target} does not contain finalized review commit ${reviewCommit.slice(0, 12)}.`,
      suggestion: `Run sprint-branch land ${input.target} ${state.sprint} before cleanup.`,
    })
  }
  if (currentBranchIsDeleted(await getCurrentBranch(rootDir), branchesToDelete)) {
    diagnostics.push({
      severity: "error",
      code: "current_branch_would_be_deleted",
      message: "The current branch is one of the sprint branches cleanup would delete.",
      suggestion: `Check out ${input.target} before cleanup.`,
    })
  }
}

async function pushSharedFinalizedDiagnostics(
  rootDir: string,
  input: HumanCommandInput & { target: string },
  state: SprintBranchState | null,
  reviewCommit: string | null,
  targetCommit: string | null,
  diagnostics: SprintDiagnostic[],
) {
  const workingTree = await getWorkingTreeStatus(rootDir)
  if (!workingTree.clean) {
    diagnostics.push({
      severity: "error",
      code: "dirty_worktree",
      message: `${input.dryRun ? "dry-run" : "command"} requires a clean working tree.`,
    })
  }
  if (!input.dryRun && input.json) {
    diagnostics.push({
      severity: "error",
      code: "interactive_json_not_supported",
      message: "land and cleanup only support --json with --dry-run.",
    })
  }
  if (!input.dryRun && (!process.stdin.isTTY || !process.stdout.isTTY)) {
    diagnostics.push({
      severity: "error",
      code: "interactive_tty_required",
      message: "land and cleanup require an interactive terminal.",
    })
  }
  if (!(await getBranchHead(rootDir, input.target))) {
    diagnostics.push({
      severity: "error",
      code: "target_branch_missing",
      message: `Target branch ${input.target} does not exist.`,
    })
  }
  if (parseSprintBranchName(input.target)) {
    diagnostics.push({
      severity: "error",
      code: "target_is_sprint_branch",
      message: `${input.target} is a sprint branch and cannot be used as a landing target.`,
    })
  }
  if (!state) {
    return
  }

  const approvedCommit = await getBranchHead(rootDir, state.branches.approved)
  const nextCommit = await getBranchHead(rootDir, state.branches.next)

  if (state.conflict) {
    diagnostics.push({
      severity: "error",
      code: "conflict_recorded",
      message: `Sprint state records an unresolved ${state.conflict.command ?? "unknown"} conflict.`,
    })
  }
  if (state.tasks.review || state.tasks.next) {
    diagnostics.push({
      severity: "error",
      code: "unreviewed_work_exists",
      message: "Landing requires no review task and no next task.",
    })
  }
  if (state.activeStashes.length > 0) {
    diagnostics.push({
      severity: "error",
      code: "active_stashes_exist",
      message: "Landing requires all sprint interruption stashes to be resumed or resolved.",
      suggestion: "Run sprint-branch resume before landing.",
    })
  }
  if (!reviewCommit) {
    diagnostics.push({
      severity: "error",
      code: "review_branch_missing",
      message: `Review branch ${state.branches.review} does not exist.`,
    })
  }
  if (!approvedCommit) {
    diagnostics.push({
      severity: "error",
      code: "approved_branch_missing",
      message: `Approved branch ${state.branches.approved} does not exist.`,
    })
  }
  if (reviewCommit && approvedCommit && reviewCommit !== approvedCommit) {
    diagnostics.push({
      severity: "error",
      code: "review_approved_mismatch",
      message: `${state.branches.review} and ${state.branches.approved} do not point at the same finalized content.`,
    })
  }
  if (nextCommit && reviewCommit && nextCommit !== reviewCommit) {
    diagnostics.push({
      severity: "error",
      code: "active_next_branch_exists",
      message: `${state.branches.next} still points at content different from review.`,
    })
  }
  if (!targetCommit) {
    diagnostics.push({
      severity: "error",
      code: "target_branch_unresolved",
      message: `Target branch ${input.target} could not be resolved.`,
    })
  }
}

function currentBranchIsDeleted(currentBranch: string | null, branchesToDelete: string[]) {
  return Boolean(currentBranch && branchesToDelete.includes(currentBranch))
}
