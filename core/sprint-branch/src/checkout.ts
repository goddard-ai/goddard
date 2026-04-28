import path from "node:path"
import { isCancel, select } from "@clack/prompts"

import { GitCommandError, runGit } from "./git/command"
import { branchExists, getBranchHead } from "./git/refs"
import { getCurrentBranch, resolveRepositoryRoot } from "./git/repository"
import { getWorkingTreeStatus } from "./git/worktree"
import { parseSprintBranchName, validateSprintName } from "./state/branches"
import { findSprintStateFiles, readSprintStateFile } from "./state/io"
import { sprintStatePath } from "./state/paths"
import type { SprintBranchState, SprintDiagnostic } from "./types"

/** Inputs needed to resolve and optionally run a detached review checkout. */
type CheckoutInput = {
  cwd: string
  sprint?: string
  dryRun: boolean
  json: boolean
}

/** One sprint review branch that can be checked out for human inspection. */
type CheckoutCandidate = {
  sprint: string
  stateRelativePath: string
  reviewBranch: string
}

/** Report returned after planning or running a human-safe review checkout. */
export type SprintCheckoutReport = {
  ok: boolean
  command: "checkout"
  dryRun: boolean
  executed: boolean
  sprint: string | null
  currentBranch: string | null
  reviewBranch: string | null
  commit: string | null
  detached: boolean
  gitOperations: string[]
  diagnostics: SprintDiagnostic[]
  candidates: Array<{
    sprint: string
    statePath: string
    reviewBranch: string
  }>
}

/** Checks out a sprint review branch as a detached human review snapshot. */
export async function runCheckout(input: CheckoutInput) {
  const rootDir = await resolveRepositoryRoot(input.cwd)
  const currentBranch = await getCurrentBranch(rootDir)
  const diagnostics: SprintDiagnostic[] = []
  const target = await resolveCheckoutTarget(rootDir, input, currentBranch, diagnostics)
  const workingTree = await getWorkingTreeStatus(rootDir)
  const reviewBranch = target?.reviewBranch ?? null
  const commit = reviewBranch ? await getBranchHead(rootDir, reviewBranch) : null
  const gitOperations = reviewBranch ? [`git checkout --detach ${reviewBranch}`] : []

  if (!workingTree.clean) {
    diagnostics.push({
      severity: "error",
      code: "dirty_worktree",
      message: "checkout requires a clean working tree before switching review snapshots.",
    })
  }
  if (reviewBranch && !(await branchExists(rootDir, reviewBranch))) {
    diagnostics.push({
      severity: "error",
      code: "review_branch_missing",
      message: `Review branch ${reviewBranch} does not exist.`,
    })
  }

  const report = {
    ok: !diagnostics.some((diagnostic) => diagnostic.severity === "error"),
    command: "checkout" as const,
    dryRun: input.dryRun,
    executed: false,
    sprint: target?.sprint ?? null,
    currentBranch,
    reviewBranch,
    commit,
    detached: false,
    gitOperations,
    diagnostics,
    candidates: target ? [] : await checkoutCandidatesForOutput(rootDir),
  } satisfies SprintCheckoutReport

  if (input.dryRun || !report.ok || !target) {
    return report
  }

  try {
    await runGit(rootDir, ["checkout", "--detach", target.reviewBranch])
    return {
      ...report,
      executed: true,
      detached: true,
      commit: await getBranchHead(rootDir, target.reviewBranch),
    } satisfies SprintCheckoutReport
  } catch (error) {
    if (error instanceof GitCommandError) {
      return {
        ...report,
        ok: false,
        executed: true,
        diagnostics: [
          ...report.diagnostics,
          {
            severity: "error",
            code: "git_checkout_failed",
            message: error.stderr || error.message,
          },
        ],
      } satisfies SprintCheckoutReport
    }
    throw error
  }
}

/** Formats the human review checkout report for terminal output. */
export function formatCheckoutReport(report: SprintCheckoutReport) {
  const lines = [
    `${report.dryRun ? "Dry run" : report.executed ? "Executed" : "Planned"}: checkout`,
    `Sprint: ${report.sprint ?? "unknown"}`,
    `Current branch: ${report.currentBranch ?? "detached HEAD"}`,
    `Review branch: ${report.reviewBranch ?? "unknown"}`,
    `Commit: ${report.commit?.slice(0, 12) ?? "unknown"}`,
    `Detached: ${report.detached ? "yes" : "no"}`,
    "",
    "Git operations:",
    ...formatList(report.gitOperations),
  ]

  if (report.candidates.length > 0) {
    lines.push("", "Candidates:")
    for (const candidate of report.candidates) {
      lines.push(`  - ${candidate.sprint}: ${candidate.reviewBranch}`)
    }
  }

  if (report.executed && report.sprint) {
    lines.push(
      "",
      `Rerun sprint-branch checkout ${report.sprint} to refresh this snapshot after agent edits.`,
    )
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

async function resolveCheckoutTarget(
  rootDir: string,
  input: CheckoutInput,
  currentBranch: string | null,
  diagnostics: SprintDiagnostic[],
) {
  if (input.sprint) {
    return readExplicitCandidate(rootDir, input.sprint, diagnostics)
  }

  const candidates = await readCheckoutCandidates(rootDir)
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
      code: "ambiguous_sprint_checkout",
      message: "Multiple sprints are available. Pass the sprint name as an argument.",
      suggestion: "sprint-branch checkout <name>",
    })
    return null
  }

  const selected = await select({
    message: "Select sprint review snapshot",
    options: candidates.map((candidate) => ({
      value: candidate.sprint,
      label: candidate.sprint,
      hint: candidate.reviewBranch,
    })),
  })

  if (isCancel(selected)) {
    diagnostics.push({
      severity: "error",
      code: "checkout_cancelled",
      message: "Checkout cancelled.",
    })
    return null
  }

  return candidates.find((candidate) => candidate.sprint === selected) ?? null
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

function inferCandidate(
  rootDir: string,
  cwd: string,
  currentBranch: string | null,
  candidates: CheckoutCandidate[],
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

async function readCheckoutCandidates(rootDir: string) {
  const stateFiles = await findSprintStateFiles(rootDir)
  const candidates: CheckoutCandidate[] = []

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

async function checkoutCandidatesForOutput(rootDir: string) {
  return (await readCheckoutCandidates(rootDir)).map((candidate) => ({
    sprint: candidate.sprint,
    statePath: candidate.stateRelativePath,
    reviewBranch: candidate.reviewBranch,
  }))
}

function candidateFromState(
  rootDir: string,
  statePath: string,
  state: SprintBranchState,
): CheckoutCandidate {
  return {
    sprint: state.sprint,
    stateRelativePath: path.relative(rootDir, statePath),
    reviewBranch: state.branches.review,
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

function formatList(values: string[]) {
  if (values.length === 0) {
    return ["  none"]
  }
  return values.map((value) => `  - ${value}`)
}

function isMissingFileError(error: unknown) {
  return (
    typeof error === "object" &&
    error !== null &&
    "code" in error &&
    (error as { code?: unknown }).code === "ENOENT"
  )
}
