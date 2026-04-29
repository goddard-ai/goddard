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
  type MutationOutput,
} from "./support"

describe("sprint-branch finalize", () => {
  afterEach(cleanupTestRepos)

  test("leaves review checked out for the human merge", async () => {
    const repo = await createSprintRepo("example", {
      review: null,
      next: null,
      approved: ["010-task-name"],
      finishedUnreviewed: [],
    })
    const result = await runCli(repo, ["finalize", "--json"])

    expect(result.exitCode).toBe(0)
    expect(await currentBranch(repo)).toBe("sprint/example/review")
    expect((await readState(repo, "example")).baseBranch).toBe("main")
  })

  test("uses an explicit override base for recovery", async () => {
    const repo = await createSprintRepo("example", {
      review: null,
      next: null,
      approved: ["010-task-name"],
      finishedUnreviewed: [],
    })
    await git(repo, ["branch", "release", "main"])

    const result = await runCli(repo, ["finalize", "--override-base", "release", "--json"])

    expect(result.exitCode).toBe(0)
    expect((await readState(repo, "example")).baseBranch).toBe("release")
  })

  // Finalize prepares the one branch a human will merge.
  // Any recorded review, next, or finished-unreviewed task means that branch is not final yet.
  test("refuses to finalize while unreviewed work is recorded", async () => {
    const repo = await createSprintRepo("example", {
      review: "010-task-name",
      next: null,
      approved: [],
      finishedUnreviewed: [],
    })
    const beforeState = await readState(repo, "example")

    const result = await runCli(repo, ["finalize", "--json"])
    const finalize = JSON.parse(result.stdout) as MutationOutput

    expect(result.exitCode).toBe(1)
    expect(diagnosticCodes(finalize)).toContain("unreviewed_work_exists")
    expect(await currentBranch(repo)).toBe("main")
    expect(await readState(repo, "example")).toEqual(beforeState)
  })

  // Even if state says there is no next task, the next branch can still contain stray commits.
  // Finalize refuses so unreviewed work-ahead content cannot be left behind or silently ignored.
  test("refuses when an unrecorded next branch still differs from review", async () => {
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
    await git(repo, ["checkout", "sprint/example/next"])
    await fs.writeFile(path.join(repo, "next.txt"), "ahead\n")
    await commitAll(repo, "add next work")

    const result = await runCli(repo, ["finalize", "--json"])
    const finalize = JSON.parse(result.stdout) as MutationOutput

    expect(result.exitCode).toBe(1)
    expect(diagnosticCodes(finalize)).toContain("active_next_branch_exists")
    expect(await currentBranch(repo)).toBe("sprint/example/next")
  })

  // The final rebase is intentionally the last Git rewrite before human merge.
  // If it conflicts, canonical sprint state must stay unchanged so Git can continue cleanly.
  test("keeps sprint state unchanged when final rebase stops", async () => {
    const repo = await createSprintRepo("example", {
      review: null,
      next: null,
      approved: ["010-task-name"],
      finishedUnreviewed: [],
    })
    await git(repo, ["checkout", "sprint/example/review"])
    await fs.writeFile(path.join(repo, "conflict.txt"), "review\n")
    await commitAll(repo, "add review conflict")
    await git(repo, ["branch", "-f", "sprint/example/approved", "sprint/example/review"])
    await git(repo, ["checkout", "main"])
    await fs.writeFile(path.join(repo, "conflict.txt"), "main\n")
    await commitAll(repo, "add main conflict")
    expect(await branchHead(repo, "sprint/example/review")).toBe(
      await branchHead(repo, "sprint/example/approved"),
    )

    const result = await runCli(repo, ["finalize", "--json"])
    const finalize = JSON.parse(result.stdout) as MutationOutput
    const state = await readState(repo, "example")

    expect(result.exitCode).toBe(1)
    expect(finalize.ok).toBe(false)
    expect(state.conflict).toBeNull()
    expect(await branchHead(repo, "sprint/example/approved")).toBe(
      await branchHead(repo, "sprint/example/review"),
    )
  })

  // Finalize may stop after Git has started rebasing the review branch, but before
  // approved has moved. Once the agent finishes the rebase, rerunning finalize
  // should only complete the approved-ref update and clear the recorded conflict.
  test("retries finalize after final rebase conflict is resolved", async () => {
    const repo = await createSprintRepo("example", {
      review: null,
      next: null,
      approved: ["010-task-name"],
      finishedUnreviewed: [],
    })
    await git(repo, ["checkout", "sprint/example/review"])
    await fs.writeFile(path.join(repo, "conflict.txt"), "review\n")
    await commitAll(repo, "add review conflict")
    await git(repo, ["branch", "-f", "sprint/example/approved", "sprint/example/review"])
    await git(repo, ["checkout", "main"])
    await fs.writeFile(path.join(repo, "conflict.txt"), "main\n")
    await commitAll(repo, "add main conflict")

    expect((await runCli(repo, ["finalize", "--json"])).exitCode).toBe(1)
    await fs.writeFile(path.join(repo, "conflict.txt"), "resolved\n")
    await git(repo, ["add", "conflict.txt"])
    await git(repo, ["-c", "core.editor=true", "rebase", "--continue"])

    const result = await runCli(repo, ["finalize", "--json"])
    const state = await readState(repo, "example")

    expect(result.exitCode).toBe(0)
    expect(await currentBranch(repo)).toBe("sprint/example/review")
    expect(state.conflict).toBeNull()
    expect(await branchHead(repo, "sprint/example/approved")).toBe(
      await branchHead(repo, "sprint/example/review"),
    )
  })
})
