import * as fs from "node:fs/promises"
import os from "node:os"
import path from "node:path"
import { afterEach, describe, expect, test } from "bun:test"

import { executeCleanupOperations, executeLandOperations } from "../src/landing"
import { sprintStatePath } from "../src/state/paths"
import {
  branchExists,
  branchHead,
  cleanupTestRepos,
  commitAll,
  createLinkedWorktree,
  createSprintRepo,
  currentBranch,
  diagnosticCodes,
  git,
  pathExists,
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
  diagnostics: Array<{ code: string; severity?: string }>
  candidates: Array<{ sprint: string; reviewBranch: string }>
  branchesToDelete?: string[]
  worktreesToDetach?: Array<{ path: string }>
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

  test("omits unfinalized sprints from land selection candidates", async () => {
    const repo = await createFinalizedReviewAheadOfMain()
    const state = await readState(repo, "example")
    await writeState(repo, "active", {
      ...state,
      sprint: "active",
      tasks: {
        review: "010-task-name",
        next: null,
        approved: [],
        finishedUnreviewed: [],
      },
    })
    await git(repo, ["branch", "sprint/active/approved"])
    await git(repo, ["branch", "sprint/active/review"])

    const result = await runCli(repo, ["land", "main", "--dry-run", "--json"])
    const land = JSON.parse(result.stdout) as HumanCommandOutput

    expect(result.exitCode).toBe(1)
    expect(diagnosticCodes(land)).toContain("sprint_selection_required")
    expect(land.candidates.map((candidate) => candidate.sprint)).toEqual(["example"])
  })

  test("includes sprints with divergent next branches when next is ignored", async () => {
    const repo = await createFinalizedReviewAheadOfMain()
    await createDivergentNextBranch(repo, "example")

    const result = await runCli(repo, [
      "land",
      "main",
      "--ignore-next-branch",
      "--dry-run",
      "--json",
    ])
    const land = JSON.parse(result.stdout) as HumanCommandOutput

    expect(result.exitCode).toBe(1)
    expect(diagnosticCodes(land)).toContain("sprint_selection_required")
    expect(land.candidates.map((candidate) => candidate.sprint)).toEqual(["example"])
  })

  test("validates land target before requiring sprint selection", async () => {
    const repo = await createFinalizedReviewAheadOfMain()

    const result = await runCli(repo, ["land", "missing-target", "--dry-run", "--json"])
    const land = JSON.parse(result.stdout) as HumanCommandOutput

    expect(result.exitCode).toBe(1)
    expect(land.ok).toBe(false)
    expect(diagnosticCodes(land)).toContain("target_branch_missing")
    expect(diagnosticCodes(land)).not.toContain("sprint_selection_required")
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

  test("allows landing when only a dormant next branch is ignored", async () => {
    const repo = await createFinalizedReviewAheadOfMain()
    await createDivergentNextBranch(repo, "example")

    const result = await runCli(repo, [
      "land",
      "main",
      "example",
      "--ignore-next-branch",
      "--dry-run",
      "--json",
    ])
    const land = JSON.parse(result.stdout) as HumanCommandOutput
    const diagnostic = land.diagnostics.find((item) => item.code === "active_next_branch_exists")

    expect(result.exitCode).toBe(0)
    expect(land.ok).toBe(true)
    expect(diagnostic?.severity).toBe("warning")
  })

  test("keeps ignored finalized next divergence as a landing warning", async () => {
    const repo = await createFinalizedReviewAheadOfMain()
    await createDivergentNextBranch(repo, "example")
    await runCli(repo, ["finalize", "--sprint", "example", "--ignore-next-branch", "--json"])

    const result = await runCli(repo, ["land", "main", "example", "--dry-run", "--json"])
    const land = JSON.parse(result.stdout) as HumanCommandOutput
    const diagnostic = land.diagnostics.find((item) => item.code === "active_next_branch_exists")

    expect(result.exitCode).toBe(0)
    expect(land.ok).toBe(true)
    expect(diagnostic?.severity).toBe("warning")
  })

  // The persisted waiver is commit-bound. New next-branch work after finalization
  // must not inherit the earlier recovery decision.
  test("rejects landing when ignored finalized next branch moves again", async () => {
    const repo = await createFinalizedReviewAheadOfMain()
    await createDivergentNextBranch(repo, "example")
    await runCli(repo, ["finalize", "--sprint", "example", "--ignore-next-branch", "--json"])
    await git(repo, ["checkout", "sprint/example/next"])
    await fs.writeFile(path.join(repo, "later-next.txt"), "later next\n")
    await commitAll(repo, "add later next work")
    await git(repo, ["checkout", "main"])

    const result = await runCli(repo, ["land", "main", "example", "--dry-run", "--json"])
    const land = JSON.parse(result.stdout) as HumanCommandOutput
    const diagnostic = land.diagnostics.find((item) => item.code === "active_next_branch_exists")

    expect(result.exitCode).toBe(1)
    expect(land.ok).toBe(false)
    expect(diagnostic?.severity).toBe("error")
  })

  // Cleanup detaches sprint branch worktrees so branch deletion does not require
  // manual checkout changes, but detached human snapshots are left alone.
  test("plans cleanup by detaching sprint branch worktrees without removing snapshots", async () => {
    const repo = await createSprintRepo(
      "example",
      {
        review: null,
        next: null,
        approved: ["010-task-name"],
      },
      { createNextBranch: true },
    )
    const snapshot = await fs.mkdtemp(path.join(os.tmpdir(), "sprint-review-snapshot-"))
    extraPaths.push(snapshot)
    await fs.rm(snapshot, { recursive: true, force: true })
    await git(repo, ["worktree", "add", "--detach", snapshot, "sprint/example/review"])
    const branchWorktree = await fs.mkdtemp(path.join(os.tmpdir(), "sprint-review-worktree-"))
    extraPaths.push(branchWorktree)
    await fs.rm(branchWorktree, { recursive: true, force: true })
    await git(repo, ["worktree", "add", branchWorktree, "sprint/example/review"])

    const result = await runCli(repo, ["cleanup", "main", "example", "--dry-run", "--json"])
    const cleanup = JSON.parse(result.stdout) as HumanCommandOutput

    expect(result.exitCode).toBe(0)
    expect(cleanup.ok).toBe(true)
    expect(cleanup.branchesToDelete).toEqual([
      "sprint/example/review",
      "sprint/example/approved",
      "sprint/example/next",
    ])
    const worktreesToDetach = await Promise.all(
      (cleanup.worktreesToDetach ?? []).map((worktree) => fs.realpath(worktree.path)),
    )
    expect(worktreesToDetach).toContain(await fs.realpath(branchWorktree))
    expect(worktreesToDetach).not.toContain(await fs.realpath(snapshot))
    expect(cleanup.gitOperations).toContain(
      `git -C ${JSON.stringify(await fs.realpath(branchWorktree))} checkout --detach`,
    )
    expect(cleanup.gitOperations).toContain("git branch -d sprint/example/review")
    expect(cleanup.stateFilesToRemove).toEqual([".git/sprint-branch/example/state.json"])
  })

  test("allows cleanup from a clean worktree checked out on a sprint branch", async () => {
    const repo = await createSprintRepo("example", {
      review: null,
      next: null,
      approved: ["010-task-name"],
    })
    await git(repo, ["checkout", "sprint/example/review"])

    const result = await runCli(repo, ["cleanup", "main", "example", "--dry-run", "--json"])
    const cleanup = JSON.parse(result.stdout) as HumanCommandOutput
    const worktreesToDetach = await Promise.all(
      (cleanup.worktreesToDetach ?? []).map((worktree) => fs.realpath(worktree.path)),
    )

    expect(result.exitCode).toBe(0)
    expect(cleanup.ok).toBe(true)
    expect(diagnosticCodes(cleanup)).not.toContain("current_branch_would_be_deleted")
    expect(worktreesToDetach).toContain(await fs.realpath(repo))
  })

  // The interactive cleanup command confirms with a human before calling this operation.
  // Testing the confirmed operation directly keeps the prompt policy intact while still
  // proving cleanup detaches branch worktrees, then removes sprint refs and Git-private
  // metadata without leaving empty .git/sprint-branch/<sprint> directories behind.
  test("detaches sprint branch worktrees and removes sprint state when confirmed cleanup executes", async () => {
    const repo = await createSprintRepo(
      "example",
      {
        review: null,
        next: null,
        approved: ["010-task-name"],
      },
      { createNextBranch: true },
    )
    const state = await readState(repo, "example")
    const branchWorktree = await fs.mkdtemp(path.join(os.tmpdir(), "sprint-review-worktree-"))
    extraPaths.push(branchWorktree)
    await fs.rm(branchWorktree, { recursive: true, force: true })
    await git(repo, ["worktree", "add", branchWorktree, "sprint/example/review"])

    await executeCleanupOperations(
      repo,
      state,
      ["sprint/example/review", "sprint/example/approved", "sprint/example/next"],
      [
        {
          path: branchWorktree,
          head: await branchHead(repo, "sprint/example/review"),
          branch: "sprint/example/review",
          detached: false,
          reason: "branch sprint/example/review",
        },
      ],
    )

    expect(await stateFileExists(repo, "example")).toBe(false)
    expect(await pathExists(path.dirname(await sprintStatePath(repo, "example")))).toBe(false)
    expect(await branchExists(repo, "sprint/example/review")).toBe(false)
    expect(await branchExists(repo, "sprint/example/approved")).toBe(false)
    expect(await branchExists(repo, "sprint/example/next")).toBe(false)
    expect(await currentBranch(branchWorktree)).toBe("")
  })

  // Git refuses to check out a branch that another linked worktree already owns.
  // Landing should recover by applying the fast-forward in that target worktree.
  test("lands in target worktree when target branch is already checked out elsewhere", async () => {
    const repo = await createFinalizedReviewAheadOfMain()
    const linkedWorktree = await createLinkedWorktree(repo, "sprint/example/review")
    const reviewHead = await branchHead(repo, "sprint/example/review")

    await executeLandOperations(linkedWorktree, "main", "sprint/example/review")

    expect(await branchHead(repo, "main")).toBe(reviewHead)
    expect(await currentBranch(repo)).toBe("main")
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
  })
  await git(repo, ["checkout", "sprint/example/review"])
  await fs.writeFile(path.join(repo, "final.txt"), "finalized\n")
  await commitAll(repo, "add finalized sprint work")
  await git(repo, ["branch", "-f", "sprint/example/approved", "sprint/example/review"])
  await git(repo, ["checkout", "main"])
  return repo
}

async function createDivergentNextBranch(repo: string, sprint: string) {
  await git(repo, ["branch", `sprint/${sprint}/next`, `sprint/${sprint}/review`])
  await git(repo, ["checkout", `sprint/${sprint}/next`])
  await fs.writeFile(path.join(repo, "next.txt"), "stale rewritten next\n")
  await commitAll(repo, "add divergent next work")
  await git(repo, ["checkout", "main"])
}
