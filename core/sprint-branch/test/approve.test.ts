import * as fs from "node:fs/promises"
import path from "node:path"
import { afterEach, describe, expect, test } from "bun:test"

import {
  branchHead,
  cleanupTestRepos,
  commitAll,
  createSprintRepo,
  currentBranch,
  diagnosticCodes,
  git,
  readState,
  runCli,
  workingTreePorcelain,
  type MutationOutput,
} from "./support"

describe("sprint-branch approve", () => {
  afterEach(cleanupTestRepos)

  test("promotes review into approved without next work", async () => {
    const repo = await createSprintRepo("example", {
      review: "010-task-name",
      next: null,
      approved: [],
    })
    await git(repo, ["checkout", "sprint/example/review"])
    await fs.writeFile(path.join(repo, "feature.txt"), "reviewed\n")
    await commitAll(repo, "add reviewed work")

    const result = await runCli(repo, ["approve", "--sprint", "example", "--json"])

    expect(result.exitCode).toBe(0)
    const state = await readState(repo, "example")
    expect(await currentBranch(repo)).toBe("sprint/example/review")
    expect(await workingTreePorcelain(repo)).toBe("")
    expect(state.tasks.review).toBeNull()
    expect(state.tasks.approved).toEqual(["010-task-name"])
    expect(await branchHead(repo, "sprint/example/review")).toBe(
      await branchHead(repo, "sprint/example/approved"),
    )
  })

  // Approval rolls the review window forward when work-ahead exists.
  // The next task becomes the new review task so humans still see one review branch.
  test("promotes next work onto review", async () => {
    const repo = await createSprintRepo(
      "example",
      {
        review: "010-task-name",
        next: "020-task-name",
        approved: [],
      },
      { createNextBranch: true },
    )
    await git(repo, ["checkout", "sprint/example/review"])
    await fs.writeFile(path.join(repo, "review.txt"), "reviewed\n")
    await commitAll(repo, "add review work")
    await git(repo, ["checkout", "sprint/example/next"])
    await git(repo, ["rebase", "sprint/example/review"])
    await fs.writeFile(path.join(repo, "next.txt"), "ahead\n")
    await commitAll(repo, "add next work")
    await git(repo, ["checkout", "sprint/example/review"])

    const result = await runCli(repo, ["approve", "--sprint", "example", "--json"])

    expect(result.exitCode).toBe(0)
    const state = await readState(repo, "example")
    expect(await workingTreePorcelain(repo)).toBe("")
    expect(state.tasks.review).toBe("020-task-name")
    expect(state.tasks.next).toBeNull()
    expect(state.tasks.approved).toEqual(["010-task-name"])
    expect(await branchHead(repo, "sprint/example/review")).toBe(
      await branchHead(repo, "sprint/example/next"),
    )
  })

  // Agents should be able to preview approval mechanics before moving protected branches.
  // The preview must not move approved or rewrite the task mapping.
  test("dry-run approval does not move branches", async () => {
    const repo = await createSprintRepo("example", {
      review: "010-task-name",
      next: null,
      approved: [],
    })
    await git(repo, ["checkout", "sprint/example/review"])
    await fs.writeFile(path.join(repo, "feature.txt"), "reviewed\n")
    await commitAll(repo, "add reviewed work")
    const approvedHead = await branchHead(repo, "sprint/example/approved")

    const result = await runCli(repo, ["approve", "--sprint", "example", "--dry-run", "--json"])
    const approve = JSON.parse(result.stdout) as MutationOutput

    expect(result.exitCode).toBe(0)
    expect(approve.dryRun).toBe(true)
    expect(approve.executed).toBe(false)
    expect(approve.gitOperations).toContain("git merge --ff-only sprint/example/review")
    expect(await branchHead(repo, "sprint/example/approved")).toBe(approvedHead)
    expect((await readState(repo, "example")).tasks.review).toBe("010-task-name")
  })

  // A fast-forward failure happens before review work is actually approved.
  // The command can detect that shape before moving refs or writing conflict state.
  test("refuses when review cannot fast-forward into approved", async () => {
    const repo = await createSprintRepo("example", {
      review: "010-task-name",
      next: null,
      approved: [],
    })
    await git(repo, ["checkout", "sprint/example/approved"])
    await fs.writeFile(path.join(repo, "approved.txt"), "approved-only\n")
    await commitAll(repo, "diverge approved")
    await git(repo, ["checkout", "sprint/example/review"])
    await fs.writeFile(path.join(repo, "review.txt"), "reviewed\n")
    await commitAll(repo, "add reviewed work")
    const beforeState = await readState(repo, "example")
    const approvedHead = await branchHead(repo, "sprint/example/approved")

    const result = await runCli(repo, ["approve", "--sprint", "example", "--json"])
    const approve = JSON.parse(result.stdout) as MutationOutput
    const state = await readState(repo, "example")

    expect(result.exitCode).toBe(1)
    expect(approve.ok).toBe(false)
    expect(diagnosticCodes(approve)).toContain("review_not_based_on_approved")
    expect(state).toEqual(beforeState)
    expect(await branchHead(repo, "sprint/example/approved")).toBe(approvedHead)
  })

  // Approval must not mark review as approved before the dependent next rebase succeeds.
  // After the agent resolves the Git rebase, rerunning approve should finish the same transition.
  test("retries approval after next rebase conflict is resolved", async () => {
    const repo = await createSprintRepo(
      "example",
      {
        review: "010-task-name",
        next: "020-task-name",
        approved: [],
      },
      { createNextBranch: true },
    )
    const approvedHead = await branchHead(repo, "sprint/example/approved")
    await git(repo, ["checkout", "sprint/example/review"])
    await fs.writeFile(path.join(repo, "conflict.txt"), "review\n")
    await commitAll(repo, "add review conflict")
    const reviewedHead = await branchHead(repo, "sprint/example/review")
    await git(repo, ["checkout", "sprint/example/next"])
    await fs.writeFile(path.join(repo, "conflict.txt"), "next\n")
    await commitAll(repo, "add next conflict")

    const first = await runCli(repo, ["approve", "--sprint", "example", "--json"])
    const failedApprove = JSON.parse(first.stdout) as MutationOutput
    const stateAfterConflict = await readState(repo, "example")

    expect(first.exitCode).toBe(1)
    expect(failedApprove.ok).toBe(false)
    expect(stateAfterConflict.conflict).toBeNull()
    expect(stateAfterConflict.tasks.review).toBe("010-task-name")
    expect(stateAfterConflict.tasks.next).toBe("020-task-name")
    expect(stateAfterConflict.tasks.approved).toEqual([])
    expect(await branchHead(repo, "sprint/example/approved")).toBe(approvedHead)

    await fs.writeFile(path.join(repo, "conflict.txt"), "resolved\n")
    await git(repo, ["add", "conflict.txt"])
    await git(repo, ["-c", "core.editor=true", "rebase", "--continue"])

    const second = await runCli(repo, ["approve", "--sprint", "example", "--json"])
    const approvedState = await readState(repo, "example")

    expect(second.exitCode).toBe(0)
    expect(approvedState.conflict).toBeNull()
    expect(approvedState.tasks.review).toBe("020-task-name")
    expect(approvedState.tasks.next).toBeNull()
    expect(approvedState.tasks.approved).toEqual(["010-task-name"])
    expect(await branchHead(repo, "sprint/example/approved")).toBe(reviewedHead)
    expect(await branchHead(repo, "sprint/example/review")).toBe(
      await branchHead(repo, "sprint/example/next"),
    )
  })
})
