import * as fs from "node:fs/promises"
import path from "node:path"
import { afterEach, describe, expect, test } from "bun:test"

import { executeCleanupOperations } from "../src/landing"
import {
  branchExists,
  branchHead,
  cleanupTestRepos,
  commitAll,
  createBaseRepo,
  createLinkedWorktree,
  currentBranch,
  git,
  readState,
  runCli,
  stashList,
  stateFileExists,
  workingTreePorcelain,
} from "./support"

type JsonOutput = {
  ok: boolean
  executed?: boolean
  detached?: boolean
  sprint?: string
  currentBranch?: string | null
  reviewBranch?: string | null
  reviewCommit?: string | null
  output?: string
  state?: {
    tasks: {
      review: string | null
      next: string | null
      approved: string[]
      finishedUnreviewed: string[]
    }
    activeStashes: Array<unknown>
  }
  workingTree?: {
    clean: boolean
  }
  diagnostics: Array<{ severity?: string; code: string }>
  branchesToDelete?: string[]
  worktreesToRemove?: Array<{
    path: string
    head: string | null
    branch: string | null
    detached: boolean
    reason: string
  }>
}

describe("sprint-branch smoke test", () => {
  afterEach(cleanupTestRepos)

  test("covers the full agent and human sprint branch workflow", async () => {
    const primary = await createBaseRepo("example")
    await fs.writeFile(path.join(primary, "sprints", "example", "030-task-name.md"), "# Task 030\n")
    await commitAll(primary, "add third sprint task")
    const agent = await createLinkedWorktree(primary)

    // The smoke path starts from a linked agent worktree because that is the
    // operationally risky shape: the agent will move live sprint branches, while
    // the human primary worktree must still discover the same Git-private state.
    // Initializing here verifies state is written to the common Git directory,
    // not to either worktree's files.
    await expectOk(agent, ["init", "--sprint", "example", "--base", "main", "--json"])
    expect(await stateFileExists(primary, "example")).toBe(true)
    expect(await workingTreePorcelain(primary)).toBe("")
    expect(await workingTreePorcelain(agent)).toBe("")

    // Status and doctor are the commands an agent should use before deciding
    // its next transition. Running them from the primary worktree immediately
    // after agent-side init proves inference works without being on a sprint
    // branch or inside sprints/<name>.
    const initialStatus = await expectOk<JsonOutput>(primary, ["status", "--json"])
    const initialDoctor = await expectOk<JsonOutput>(primary, ["doctor", "--json"])
    expect(initialStatus.sprint).toBe("example")
    expect(initialDoctor.ok).toBe(true)

    // The first start must choose the review branch automatically. This is the
    // oldest unreviewed task, and the agent should not have to name the branch
    // role. The clean status assertion protects the Git-metadata state storage
    // contract from regressing into tracked bookkeeping files.
    await expectOk(agent, ["start", "--task", "010-task-name", "--json"])
    expect(await currentBranch(agent)).toBe("sprint/example/review")
    expect(await workingTreePorcelain(agent)).toBe("")
    await fs.writeFile(path.join(agent, "feature-one.txt"), "one\n")
    await commitAll(agent, "complete first task")

    // Diff is the review surface for humans and agents. The critical property
    // is not just that git diff runs, but that the CLI preserves the approved to
    // review range after the branch-moving commands have chosen the branch roles.
    const diff = await expectOk<JsonOutput>(primary, ["diff", "--json"])
    expect(diff.output).toContain("feature-one.txt")

    // A second start while review is occupied must create work-ahead on next.
    // This branch depends on review, so later feedback can force a rebase
    // without exposing unapproved work on approved.
    await expectOk(agent, ["start", "--task", "020-task-name", "--json"])
    expect(await currentBranch(agent)).toBe("sprint/example/next")
    await fs.writeFile(path.join(agent, "feature-two.txt"), "two\n")
    await commitAll(agent, "complete second task")
    const withNextDoctor = await expectOk<JsonOutput>(primary, ["doctor", "--json"])
    expect(withNextDoctor.ok).toBe(true)

    // Human feedback interrupts work-ahead. The untracked scratch file is
    // intentional: Git's default stash would leave it behind, so the workflow
    // relies on feedback stashing untracked files and recording a resumable
    // sprint-specific stash entry before checking out review.
    await fs.writeFile(path.join(agent, "scratch-note.txt"), "carry this through feedback\n")
    await expectOk(agent, ["feedback", "--json"])
    expect(await currentBranch(agent)).toBe("sprint/example/review")
    expect(await workingTreePorcelain(agent)).toBe("")
    expect(await stashList(agent)).toContain("sprint-branch:example:020-task-name:feedback")
    expect((await readState(primary, "example")).activeStashes).toHaveLength(1)

    // Feedback changes the review branch underneath next. Resume must rebase
    // next onto the updated review branch and then reapply the recorded stash.
    // Committing the restored scratch file turns the dirty resumed work into
    // actual second-task content so approve can safely run afterward.
    await fs.writeFile(path.join(agent, "feature-one.txt"), "one\nreview feedback\n")
    await commitAll(agent, "apply review feedback")
    await expectOk(agent, ["resume", "--json"])
    expect(await currentBranch(agent)).toBe("sprint/example/next")
    expect(await fs.readFile(path.join(agent, "scratch-note.txt"), "utf-8")).toBe(
      "carry this through feedback\n",
    )
    await commitAll(agent, "commit resumed scratch work")

    // Approving the first task is the workflow's central promotion step. It
    // must fast-forward approved to review, then promote the already-rebased
    // next branch into the new review branch without requiring the agent to
    // manually reset either branch.
    await expectOk(agent, ["approve", "--json"])
    let state = await readState(primary, "example")
    expect(state.tasks).toMatchObject({
      review: "020-task-name",
      next: null,
      approved: ["010-task-name"],
    })
    expect(await branchHead(primary, "sprint/example/review")).toBe(
      await branchHead(primary, "sprint/example/next"),
    )

    // Starting the third task after promotion should recreate next from the new
    // review branch. This protects the rolling two-slot model: review carries
    // the oldest unapproved task, and next carries at most one work-ahead task.
    await expectOk(agent, ["start", "--task", "030-task-name", "--json"])
    expect(await currentBranch(agent)).toBe("sprint/example/next")
    await fs.writeFile(path.join(agent, "feature-three.txt"), "three\n")
    await commitAll(agent, "complete third task")

    // These two approvals drain the remaining review window. The first rolls
    // the third task from next into review; the second leaves no unreviewed task
    // recorded, which is the precondition for finalize and human landing.
    await expectOk(agent, ["approve", "--json"])
    state = await readState(primary, "example")
    expect(state.tasks).toMatchObject({
      review: "030-task-name",
      next: null,
      approved: ["010-task-name", "020-task-name"],
    })

    await expectOk(agent, ["approve", "--json"])
    state = await readState(primary, "example")
    expect(state.tasks).toMatchObject({
      review: null,
      next: null,
      approved: ["010-task-name", "020-task-name", "030-task-name"],
      finishedUnreviewed: [],
    })

    // Finalize is the last agent-owned branch rewrite. It must leave the review
    // branch checked out for the agent, make review and approved represent the
    // same finalized content, and keep state-only updates out of the worktree.
    await expectOk(agent, ["finalize", "--json"])
    expect(await currentBranch(agent)).toBe("sprint/example/review")
    expect(await workingTreePorcelain(agent)).toBe("")
    const reviewHead = await branchHead(primary, "sprint/example/review")
    expect(await branchHead(primary, "sprint/example/approved")).toBe(reviewHead)
    expect(await branchHead(primary, "sprint/example/next")).toBe(reviewHead)

    // Human checkout intentionally detaches HEAD in the primary worktree. The
    // live review branch remains agent-owned, and the human can refresh their
    // snapshot by rerunning checkout if the agent makes more edits.
    const checkout = await expectOk<JsonOutput>(primary, ["checkout", "example", "--json"])
    expect(checkout.detached).toBe(true)
    expect((await git(primary, ["rev-parse", "--abbrev-ref", "HEAD"])).trim()).toBe("HEAD")
    expect(await branchHead(primary, "HEAD")).toBe(reviewHead)

    // Land is human-confirmed in production, so the smoke test exercises the
    // non-interactive dry-run contract and then performs the equivalent
    // fast-forward with Git. That gives cleanup a landed target without
    // weakening the CLI's TTY-only mutation rule.
    const land = await expectOk<JsonOutput>(primary, [
      "land",
      "main",
      "example",
      "--dry-run",
      "--json",
    ])
    expect(land.reviewCommit).toBe(reviewHead)
    await git(primary, ["checkout", "main"])
    await git(primary, ["merge", "--ff-only", "sprint/example/review"])
    expect(await branchHead(primary, "main")).toBe(reviewHead)

    // Cleanup has two layers worth checking: the CLI planning path must identify
    // the checked-out agent worktree before deleting branches, and the confirmed
    // operation must remove both refs and Git-private state. Passing the agent
    // worktree to executeCleanupOperations mirrors what the interactive command
    // would do after human confirmation.
    const cleanup = await expectOk<JsonOutput>(primary, [
      "cleanup",
      "main",
      "example",
      "--dry-run",
      "--json",
    ])
    expect(cleanup.branchesToDelete).toEqual([
      "sprint/example/review",
      "sprint/example/approved",
      "sprint/example/next",
    ])
    const cleanupWorktrees = cleanup.worktreesToRemove ?? []
    expect(await realpaths(cleanupWorktrees.map((worktree) => worktree.path))).toContain(
      await fs.realpath(agent),
    )
    await executeCleanupOperations(
      primary,
      await readState(primary, "example"),
      cleanup.branchesToDelete ?? [],
      cleanupWorktrees,
    )
    expect(await stateFileExists(primary, "example")).toBe(false)
    expect(await branchExists(primary, "sprint/example/review")).toBe(false)
    expect(await branchExists(primary, "sprint/example/approved")).toBe(false)
    expect(await branchExists(primary, "sprint/example/next")).toBe(false)
  })
})

async function expectOk<T extends JsonOutput = JsonOutput>(cwd: string, args: string[]) {
  const result = await runCli(cwd, args)
  if (result.exitCode !== 0) {
    throw new Error(
      [
        `sprint-branch ${args.join(" ")} failed with exit code ${result.exitCode}`,
        result.stdout,
        result.stderr,
      ].join("\n"),
    )
  }

  expect(result.exitCode).toBe(0)
  expect(result.stderr).toBe("")

  const output = JSON.parse(result.stdout) as T
  expect(output.ok).toBe(true)
  return output
}

async function realpaths(paths: string[]) {
  return Promise.all(paths.map((entry) => fs.realpath(entry)))
}
