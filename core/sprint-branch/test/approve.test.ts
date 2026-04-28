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

    const result = await runCli(repo, ["approve", "--verified", "--json"])

    expect(result.exitCode).toBe(0)
    const state = await readState(repo, "example")
    expect(await currentBranch(repo)).toBe("sprint/example/review")
    expect(state.tasks.review).toBeNull()
    expect(state.tasks.approved).toEqual(["010-task-name"])
    expect(await branchHead(repo, "sprint/example/review")).toBe(
      await branchHead(repo, "sprint/example/approved"),
    )
  })

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

    const result = await runCli(repo, ["approve", "--verified", "--json"])

    expect(result.exitCode).toBe(0)
    const state = await readState(repo, "example")
    expect(state.tasks.review).toBe("020-task-name")
    expect(state.tasks.next).toBeNull()
    expect(state.tasks.approved).toEqual(["010-task-name"])
    expect(await branchHead(repo, "sprint/example/review")).toBe(
      await branchHead(repo, "sprint/example/next"),
    )
  })
})
