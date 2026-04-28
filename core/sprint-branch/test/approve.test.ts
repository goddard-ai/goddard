import * as fs from "node:fs/promises"
import path from "node:path"
import { afterEach, describe, expect, test } from "bun:test"

import {
  branchHead,
  cleanupTestRepos,
  commitAll,
  createSprintRepo,
  currentBranch,
  git,
  readState,
  runCli,
  type MutationOutput,
} from "./support"

describe("sprint-branch approve", () => {
  afterEach(cleanupTestRepos)

  test("promotes review into approved without next work", async () => {
    const repo = await createSprintRepo("example", {
      review: "010-task-name",
      next: null,
      approved: [],
      finishedUnreviewed: [],
    })
    await git(repo, ["checkout", "sprint/example/review"])
    await fs.writeFile(path.join(repo, "feature.txt"), "reviewed\n")
    await commitAll(repo, "add reviewed work")

    const result = await runCli(repo, ["approve", "--json"])

    expect(result.exitCode).toBe(0)
    const state = await readState(repo, "example")
    expect(await currentBranch(repo)).toBe("sprint/example/review")
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
        finishedUnreviewed: [],
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

    const result = await runCli(repo, ["approve", "--json"])

    expect(result.exitCode).toBe(0)
    const state = await readState(repo, "example")
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
      finishedUnreviewed: [],
    })
    await git(repo, ["checkout", "sprint/example/review"])
    await fs.writeFile(path.join(repo, "feature.txt"), "reviewed\n")
    await commitAll(repo, "add reviewed work")
    const approvedHead = await branchHead(repo, "sprint/example/approved")

    const result = await runCli(repo, ["approve", "--dry-run", "--json"])
    const approve = JSON.parse(result.stdout) as MutationOutput

    expect(result.exitCode).toBe(0)
    expect(approve.dryRun).toBe(true)
    expect(approve.executed).toBe(false)
    expect(approve.gitOperations).toContain("git merge --ff-only sprint/example/review")
    expect(await branchHead(repo, "sprint/example/approved")).toBe(approvedHead)
    expect((await readState(repo, "example")).tasks.review).toBe("010-task-name")
  })

  // A fast-forward failure happens before review work is actually approved.
  // The state must remain pre-approval while still recording where recovery needs to begin.
  test("records conflict state when fast-forward approval fails", async () => {
    const repo = await createSprintRepo("example", {
      review: "010-task-name",
      next: null,
      approved: [],
      finishedUnreviewed: [],
    })
    await git(repo, ["checkout", "sprint/example/approved"])
    await fs.writeFile(path.join(repo, "approved.txt"), "approved-only\n")
    await commitAll(repo, "diverge approved")
    await git(repo, ["checkout", "sprint/example/review"])
    await fs.writeFile(path.join(repo, "review.txt"), "reviewed\n")
    await commitAll(repo, "add reviewed work")

    const result = await runCli(repo, ["approve", "--json"])
    const approve = JSON.parse(result.stdout) as MutationOutput
    const state = await readState(repo, "example")

    expect(result.exitCode).toBe(1)
    expect(approve.ok).toBe(false)
    expect(state.conflict?.command).toBe("approve")
    expect(state.conflict?.branch).toBe("sprint/example/approved")
    expect(state.tasks.review).toBe("010-task-name")
    expect(state.tasks.approved).toEqual([])
  })
})
