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
  stashList,
  writeState,
  type MutationOutput,
} from "./support"

describe("sprint-branch feedback", () => {
  afterEach(cleanupTestRepos)

  test("stashes dirty next work and checks out review", async () => {
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

    const result = await runCli(repo, ["feedback", "--include-untracked", "--json"])

    expect(result.exitCode).toBe(0)
    expect(await currentBranch(repo)).toBe("sprint/example/review")
    expect((await readState(repo, "example")).activeStashes[0]?.sourceBranch).toBe(
      "sprint/example/next",
    )
    expect(await stashList(repo)).toContain("sprint-branch:example:020-task-name:feedback")
  })

  test("refuses dirty work outside the recorded next branch", async () => {
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
    await fs.writeFile(path.join(repo, "scratch.txt"), "review work\n")
    const beforeState = await readState(repo, "example")

    const result = await runCli(repo, ["feedback", "--include-untracked", "--json"])
    const feedback = JSON.parse(result.stdout) as MutationOutput

    expect(result.exitCode).toBe(1)
    expect(diagnosticCodes(feedback)).toContain("dirty_non_next_worktree")
    expect(await currentBranch(repo)).toBe("sprint/example/review")
    expect(await stashList(repo)).toBe("")
    expect(await readState(repo, "example")).toEqual(beforeState)
  })

  test("refuses untracked next work without include-untracked", async () => {
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
    const beforeState = await readState(repo, "example")

    const result = await runCli(repo, ["feedback", "--json"])
    const feedback = JSON.parse(result.stdout) as MutationOutput

    expect(result.exitCode).toBe(1)
    expect(diagnosticCodes(feedback)).toContain("untracked_work_requires_flag")
    expect(await currentBranch(repo)).toBe("sprint/example/next")
    expect(await stashList(repo)).toBe("")
    expect(await readState(repo, "example")).toEqual(beforeState)
  })

  test("stashes tracked next work without include-untracked", async () => {
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
    await fs.writeFile(path.join(repo, "README.md"), "# Test\ntracked interruption\n")

    const result = await runCli(repo, ["feedback", "--json"])
    const state = await readState(repo, "example")

    expect(result.exitCode).toBe(0)
    expect(await currentBranch(repo)).toBe("sprint/example/review")
    expect(state.activeStashes).toHaveLength(1)
    expect(state.activeStashes[0]).toMatchObject({
      sourceBranch: "sprint/example/next",
      task: "020-task-name",
      reason: "feedback",
      message: "sprint-branch:example:020-task-name:feedback",
    })
    expect(state.activeStashes[0]?.ref).toBe("stash@{0}")
    expect(await stashList(repo)).toContain("sprint-branch:example:020-task-name:feedback")
  })

  test("refuses next branch feedback when no next task is recorded", async () => {
    const repo = await createSprintRepo(
      "example",
      {
        review: "010-task-name",
        next: null,
        approved: [],
        finishedUnreviewed: [],
      },
      { createNextBranch: true },
    )
    await git(repo, ["checkout", "sprint/example/next"])
    await fs.writeFile(path.join(repo, "README.md"), "# Test\norphan next work\n")
    const beforeState = await readState(repo, "example")

    const result = await runCli(repo, ["feedback", "--json"])
    const feedback = JSON.parse(result.stdout) as MutationOutput

    expect(result.exitCode).toBe(1)
    expect(diagnosticCodes(feedback)).toContain("next_task_missing")
    expect(await currentBranch(repo)).toBe("sprint/example/next")
    expect(await stashList(repo)).toBe("")
    expect(await readState(repo, "example")).toEqual(beforeState)
  })

  test("dry-run reports the stash and checkout without mutating state", async () => {
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
    const beforeState = await readState(repo, "example")

    const result = await runCli(repo, ["feedback", "--include-untracked", "--dry-run", "--json"])
    const feedback = JSON.parse(result.stdout) as MutationOutput

    expect(result.exitCode).toBe(0)
    expect(feedback.dryRun).toBe(true)
    expect(feedback.executed).toBe(false)
    expect(feedback.gitOperations).toEqual([
      'git stash push --include-untracked -m "sprint-branch:example:020-task-name:feedback"',
      "git checkout sprint/example/review",
    ])
    expect(await currentBranch(repo)).toBe("sprint/example/next")
    expect(await stashList(repo)).toBe("")
    expect(await readState(repo, "example")).toEqual(beforeState)
  })

  test("checks out review without stashing when next is clean", async () => {
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

    const result = await runCli(repo, ["feedback", "--json"])
    const state = await readState(repo, "example")

    expect(result.exitCode).toBe(0)
    expect(await currentBranch(repo)).toBe("sprint/example/review")
    expect(await stashList(repo)).toBe("")
    expect(state.activeStashes).toEqual([])
  })

  test("appends new feedback stashes to existing active stash records", async () => {
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
    const state = await readState(repo, "example")
    const existingStash = {
      ref: "stash@{99}",
      sourceBranch: "sprint/example/next",
      task: "020-task-name",
      reason: "feedback",
      message: "sprint-branch:example:020-task-name:feedback:old",
    }
    await writeState(repo, "example", {
      ...state,
      activeStashes: [existingStash],
    })
    await commitAll(repo, "seed active stash")
    await git(repo, ["branch", "-f", "sprint/example/approved", "HEAD"])
    await git(repo, ["branch", "-f", "sprint/example/review", "HEAD"])
    await git(repo, ["branch", "-f", "sprint/example/next", "HEAD"])
    await git(repo, ["checkout", "sprint/example/next"])
    await fs.writeFile(path.join(repo, "README.md"), "# Test\ntracked interruption\n")

    const result = await runCli(repo, ["feedback", "--json"])
    const nextState = await readState(repo, "example")

    expect(result.exitCode).toBe(0)
    expect(nextState.activeStashes).toHaveLength(2)
    expect(nextState.activeStashes[0]).toEqual(existingStash)
    expect(nextState.activeStashes[1]).toMatchObject({
      ref: "stash@{0}",
      sourceBranch: "sprint/example/next",
      task: "020-task-name",
      reason: "feedback",
      message: "sprint-branch:example:020-task-name:feedback",
    })
  })
})
