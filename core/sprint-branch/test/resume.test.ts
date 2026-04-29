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
  workingTreePorcelain,
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
    expect((await runCli(repo, ["feedback"])).exitCode).toBe(0)
    await commitAll(repo, "record feedback transition")

    const result = await runCli(repo, ["resume", "--json"])

    expect(result.exitCode).toBe(0)
    expect(await currentBranch(repo)).toBe("sprint/example/next")
    expect(await fs.readFile(path.join(repo, "scratch.txt"), "utf-8")).toBe("interrupted\n")
    expect((await readState(repo, "example")).activeStashes).toEqual([])
  })

  // Resume is the command most likely to hit a real Git conflict after review feedback.
  // While Git is mid-rebase, canonical sprint state must stay untouched so rebase --continue works.
  test("keeps sprint state unchanged when rebase stops", async () => {
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
    expect(state.conflict).toBeNull()
    expect(state.tasks.review).toBe("010-task-name")
    expect(state.tasks.next).toBe("020-task-name")
  })

  // Resume is retryable after a manual rebase resolution because sprint state
  // still describes the original next task until the command completes successfully.
  test("retries resume after rebase conflict is resolved", async () => {
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

    expect((await runCli(repo, ["resume", "--json"])).exitCode).toBe(1)
    await fs.writeFile(path.join(repo, "conflict.txt"), "resolved\n")
    await git(repo, ["add", "conflict.txt"])
    await git(repo, ["-c", "core.editor=true", "rebase", "--continue"])

    const result = await runCli(repo, ["resume", "--json"])
    const state = await readState(repo, "example")

    expect(result.exitCode).toBe(0)
    expect(await currentBranch(repo)).toBe("sprint/example/next")
    expect(await workingTreePorcelain(repo)).toBe("")
    expect(state.conflict).toBeNull()
    expect(state.tasks.next).toBe("020-task-name")
  })

  // Stash application conflicts are different from rebase conflicts: Git has no
  // follow-up "continue" command, and a successful resume intentionally leaves
  // the restored work dirty. Rerunning resume after conflicts are resolved should
  // only clear the recorded stash instead of applying it again.
  test("finishes resume after a stash-apply conflict is resolved", async () => {
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
    await fs.writeFile(path.join(repo, "work.txt"), "base\n")
    await commitAll(repo, "add shared work file")
    await git(repo, ["checkout", "sprint/example/next"])
    await git(repo, ["rebase", "sprint/example/review"])
    await fs.writeFile(path.join(repo, "work.txt"), "stashed\n")
    expect((await runCli(repo, ["feedback"])).exitCode).toBe(0)
    await git(repo, ["checkout", "sprint/example/review"])
    await fs.writeFile(path.join(repo, "work.txt"), "review\n")
    await commitAll(repo, "change work during feedback")

    const first = await runCli(repo, ["resume", "--json"])
    const conflictState = await readState(repo, "example")

    expect(first.exitCode).toBe(1)
    expect(conflictState.conflict?.command).toBe("resume")
    expect(conflictState.activeStashes).toHaveLength(1)

    await fs.writeFile(path.join(repo, "work.txt"), "resolved\n")
    await git(repo, ["add", "work.txt"])

    const second = await runCli(repo, ["resume", "--json"])
    const state = await readState(repo, "example")

    expect(second.exitCode).toBe(0)
    expect(await currentBranch(repo)).toBe("sprint/example/next")
    expect(await fs.readFile(path.join(repo, "work.txt"), "utf-8")).toBe("resolved\n")
    expect(state.conflict).toBeNull()
    expect(state.activeStashes).toEqual([])
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
    expect(await workingTreePorcelain(repo)).toBe("")
    expect((await readState(repo, "example")).activeStashes).toEqual([])
  })
})
