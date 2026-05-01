import * as fs from "node:fs/promises"
import os from "node:os"
import path from "node:path"
import { afterEach, describe, expect, test } from "bun:test"

import { executeCleanupOperations } from "../src/landing"
import {
  branchExists,
  branchHead,
  cleanupTestRepos,
  commitAll,
  createSprintRepo,
  currentBranch,
  diagnosticCodes,
  git,
  readState,
  runCli,
  stateFileExists,
  writeState,
} from "./support"

/** Machine-readable payload shared by land and cleanup CLI tests. */
type HumanCommandOutput = {
  ok: boolean
  dryRun: boolean
  executed: boolean
  sprint: string | null
  targetBranch: string
  reviewBranch: string | null
  reviewCommit: string | null
  gitOperations: string[]
  diagnostics: Array<{ code: string }>
  candidates: Array<{ sprint: string; reviewBranch: string }>
  branchesToDelete?: string[]
  worktreesToRemove?: Array<{ path: string }>
  stateFilesToRemove?: string[]
}

const extraPaths: string[] = []

describe("sprint-branch human landing commands", () => {
  afterEach(async () => {
    await cleanupTestRepos()
    await Promise.all(
      extraPaths.splice(0).map((entry) => fs.rm(entry, { recursive: true, force: true })),
    )
  })

  test("plans a fast-forward land from target to finalized review", async () => {
    const repo = await createFinalizedReviewAheadOfMain()
    const mainHead = await branchHead(repo, "main")
    const reviewHead = await branchHead(repo, "sprint/example/review")

    const result = await runCli(repo, ["land", "main", "example", "--dry-run", "--json"])
    const land = JSON.parse(result.stdout) as HumanCommandOutput

    expect(result.exitCode).toBe(0)
    expect(land.ok).toBe(true)
    expect(land.dryRun).toBe(true)
    expect(land.executed).toBe(false)
    expect(land.reviewCommit).toBe(reviewHead)
    expect(land.gitOperations).toEqual([
      "git checkout main",
      "git merge --ff-only sprint/example/review",
    ])
    expect(await branchHead(repo, "main")).toBe(mainHead)
  })

  test("refuses non-interactive land without a strong sprint context", async () => {
    const repo = await createFinalizedReviewAheadOfMain()

    const result = await runCli(repo, ["land", "main", "--dry-run", "--json"])
    const land = JSON.parse(result.stdout) as HumanCommandOutput

    expect(result.exitCode).toBe(1)
    expect(land.ok).toBe(false)
    expect(diagnosticCodes(land)).toContain("sprint_selection_required")
    expect(land.candidates.map((candidate) => candidate.sprint)).toEqual(["example"])
  })

  // Landing changes the branch humans ultimately merge from, so it must never be
  // run by an unattended agent or script until an explicit automation policy exists.
  test("refuses non-interactive land mutation", async () => {
    const repo = await createFinalizedReviewAheadOfMain()
    const mainHead = await branchHead(repo, "main")

    const result = await runCli(repo, ["land", "main", "example"])

    expect(result.exitCode).toBe(1)
    expect(result.stdout).toContain("interactive_tty_required")
    expect(await branchHead(repo, "main")).toBe(mainHead)
  })

  // Human landing is only valid after the sprint review branch has been finalized.
  // This prevents merging the active review window before approval/promotion is complete.
  test("refuses to land while unreviewed work is recorded", async () => {
    const repo = await createSprintRepo("example", {
      review: "010-task-name",
      next: null,
      approved: [],
      finishedUnreviewed: [],
    })

    const result = await runCli(repo, ["land", "main", "example", "--dry-run", "--json"])
    const land = JSON.parse(result.stdout) as HumanCommandOutput

    expect(result.exitCode).toBe(1)
    expect(diagnosticCodes(land)).toContain("unreviewed_work_exists")
  })

  // A recorded stash means next-branch work was interrupted and has not been
  // reconciled, even when the visible task slots otherwise look finalized.
  test("refuses to land while active sprint stashes are recorded", async () => {
    const repo = await createFinalizedReviewAheadOfMain()
    const state = await readState(repo, "example")
    await writeState(repo, "example", {
      ...state,
      activeStashes: [
        {
          ref: "stash@{0}",
          sourceBranch: "sprint/example/next",
          task: "020-task-name",
          reason: "feedback",
          message: "sprint-branch example feedback 020-task-name",
        },
      ],
    })

    const result = await runCli(repo, ["land", "main", "example", "--dry-run", "--json"])
    const land = JSON.parse(result.stdout) as HumanCommandOutput

    expect(result.exitCode).toBe(1)
    expect(diagnosticCodes(land)).toContain("active_stashes_exist")
  })

  // Cleanup can remove detached human snapshots, but only when the target branch
  // already contains the finalized review commit and all associated worktrees are clean.
  test("plans cleanup of landed sprint branches and detached review worktree", async () => {
    const repo = await createSprintRepo(
      "example",
      {
        review: null,
        next: null,
        approved: ["010-task-name"],
        finishedUnreviewed: [],
      },
      { createNextBranch: true },
    )
    const snapshot = await fs.mkdtemp(path.join(os.tmpdir(), "sprint-review-snapshot-"))
    extraPaths.push(snapshot)
    await fs.rm(snapshot, { recursive: true, force: true })
    await git(repo, ["worktree", "add", "--detach", snapshot, "sprint/example/review"])

    const result = await runCli(repo, ["cleanup", "main", "example", "--dry-run", "--json"])
    const cleanup = JSON.parse(result.stdout) as HumanCommandOutput

    expect(result.exitCode).toBe(0)
    expect(cleanup.ok).toBe(true)
    expect(cleanup.branchesToDelete).toEqual([
      "sprint/example/review",
      "sprint/example/approved",
      "sprint/example/next",
    ])
    const worktreePaths = await Promise.all(
      (cleanup.worktreesToRemove ?? []).map((worktree) => fs.realpath(worktree.path)),
    )
    expect(worktreePaths).toContain(await fs.realpath(snapshot))
    expect(cleanup.gitOperations).toContain("git branch -d sprint/example/review")
    expect(cleanup.stateFilesToRemove).toEqual([".git/sprint-branch/example/state.json"])
  })

  // The interactive cleanup command confirms with a human before calling this operation.
  // Testing the confirmed operation directly keeps the prompt policy intact while still
  // proving cleanup removes the Git-private state file along with sprint refs.
  test("removes sprint state when confirmed cleanup executes", async () => {
    const repo = await createSprintRepo(
      "example",
      {
        review: null,
        next: null,
        approved: ["010-task-name"],
        finishedUnreviewed: [],
      },
      { createNextBranch: true },
    )
    const state = await readState(repo, "example")

    await executeCleanupOperations(
      repo,
      state,
      ["sprint/example/review", "sprint/example/approved", "sprint/example/next"],
      [],
    )

    expect(await stateFileExists(repo, "example")).toBe(false)
    expect(await branchExists(repo, "sprint/example/review")).toBe(false)
    expect(await branchExists(repo, "sprint/example/approved")).toBe(false)
    expect(await branchExists(repo, "sprint/example/next")).toBe(false)
  })

  // Cleanup is destructive, so target containment is the key proof that deleting
  // sprint refs will not discard the finalized review commit.
  test("refuses cleanup before target contains review", async () => {
    const repo = await createFinalizedReviewAheadOfMain()

    const result = await runCli(repo, ["cleanup", "main", "example", "--dry-run", "--json"])
    const cleanup = JSON.parse(result.stdout) as HumanCommandOutput

    expect(result.exitCode).toBe(1)
    expect(diagnosticCodes(cleanup)).toContain("target_missing_review")
  })

  test("refuses non-interactive cleanup mutation", async () => {
    const repo = await createSprintRepo("example", {
      review: null,
      next: null,
      approved: ["010-task-name"],
      finishedUnreviewed: [],
    })

    const result = await runCli(repo, ["cleanup", "main", "example"])

    expect(result.exitCode).toBe(1)
    expect(result.stdout).toContain("interactive_tty_required")
    expect(await currentBranch(repo)).toBe("main")
  })
})

async function createFinalizedReviewAheadOfMain() {
  const repo = await createSprintRepo("example", {
    review: null,
    next: null,
    approved: ["010-task-name"],
    finishedUnreviewed: [],
  })
  await git(repo, ["checkout", "sprint/example/review"])
  await fs.writeFile(path.join(repo, "final.txt"), "finalized\n")
  await commitAll(repo, "add finalized sprint work")
  await git(repo, ["branch", "-f", "sprint/example/approved", "sprint/example/review"])
  await git(repo, ["checkout", "main"])
  return repo
}
