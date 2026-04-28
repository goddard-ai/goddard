import * as fs from "node:fs/promises"
import path from "node:path"
import { afterEach, describe, expect, test } from "bun:test"

import {
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

describe("sprint-branch resume", () => {
  afterEach(cleanupTestRepos)

  test("rebases next and applies a recorded feedback stash", async () => {
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
    await git(repo, ["checkout", "sprint/example/next"])
    await fs.writeFile(path.join(repo, "scratch.txt"), "interrupted\n")
    expect((await runCli(repo, ["feedback", "--include-untracked"])).exitCode).toBe(0)
    await commitAll(repo, "record feedback transition")

    const result = await runCli(repo, ["resume", "--json"])

    expect(result.exitCode).toBe(0)
    expect(await currentBranch(repo)).toBe("sprint/example/next")
    expect(await fs.readFile(path.join(repo, "scratch.txt"), "utf-8")).toBe("interrupted\n")
    expect((await readState(repo, "example")).activeStashes).toEqual([])
  })

  // Resume is the command most likely to hit a real Git conflict after review feedback.
  // Persisting conflict state lets the next agent know which sprint transition stopped midway.
  test("records conflict state when rebase stops", async () => {
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
    await git(repo, ["checkout", "sprint/example/next"])
    await fs.writeFile(path.join(repo, "conflict.txt"), "next\n")
    await commitAll(repo, "add next conflict")
    await git(repo, ["checkout", "sprint/example/review"])
    await fs.writeFile(path.join(repo, "conflict.txt"), "review\n")
    await commitAll(repo, "add review conflict")

    const result = await runCli(repo, ["resume", "--json"])

    expect(result.exitCode).toBe(1)
    const report = JSON.parse(result.stdout) as { ok: boolean }
    const state = await readState(repo, "example")
    expect(report.ok).toBe(false)
    expect(state.conflict?.command).toBe("resume")
    expect(state.conflict?.branch).toBe("sprint/example/next")
  })

  // Resume may rebase and apply a saved stash, both of which assume a clean target.
  // Refusing early prevents Git from mixing old local edits with resumed work-ahead changes.
  test("refuses to resume with a dirty worktree", async () => {
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
    await fs.writeFile(path.join(repo, "README.md"), "# Test\ndirty\n")
    const beforeState = await readState(repo, "example")

    const result = await runCli(repo, ["resume", "--json"])
    const resume = JSON.parse(result.stdout) as MutationOutput

    expect(result.exitCode).toBe(1)
    expect(diagnosticCodes(resume)).toContain("dirty_worktree")
    expect(await currentBranch(repo)).toBe("main")
    expect(await readState(repo, "example")).toEqual(beforeState)
  })

  // A recorded next task without the next branch means the canonical state and Git refs diverged.
  // Resume must stop before it accidentally continues on review or recreates history incorrectly.
  test("refuses when a recorded next task has no next branch", async () => {
    const repo = await createSprintRepo("example", {
      review: "010-task-name",
      next: "020-task-name",
      approved: [],
      finishedUnreviewed: [],
    })
    const beforeState = await readState(repo, "example")

    const result = await runCli(repo, ["resume", "--json"])
    const resume = JSON.parse(result.stdout) as MutationOutput

    expect(result.exitCode).toBe(1)
    expect(diagnosticCodes(resume)).toContain("next_branch_missing")
    expect(await currentBranch(repo)).toBe("main")
    expect(await readState(repo, "example")).toEqual(beforeState)
  })

  // Resume is also the safe "return me to work" command when no work-ahead exists.
  // In that state the only valid continuation point is the review branch.
  test("checks out review when no next task is recorded", async () => {
    const repo = await createSprintRepo("example", {
      review: "010-task-name",
      next: null,
      approved: [],
      finishedUnreviewed: [],
    })

    const result = await runCli(repo, ["resume", "--json"])

    expect(result.exitCode).toBe(0)
    expect(await currentBranch(repo)).toBe("sprint/example/review")
    expect((await readState(repo, "example")).activeStashes).toEqual([])
  })
})
