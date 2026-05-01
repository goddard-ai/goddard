import * as fs from "node:fs/promises"
import path from "node:path"
import { afterEach, describe, expect, test } from "bun:test"

import {
  branchExists,
  cleanupTestRepos,
  commitAll,
  createSprintRepo,
  currentBranch,
  diagnosticCodes,
  readState,
  runCli,
  workingTreePorcelain,
  type MutationOutput,
} from "./support"

describe("sprint-branch start", () => {
  afterEach(cleanupTestRepos)

  test("places the first task on review", async () => {
    const repo = await createSprintRepo("example", {
      review: null,
      next: null,
      approved: [],
    })
    const result = await runCli(repo, [
      "start",
      "--sprint",
      "example",
      "--task",
      "010-task-name",
      "--json",
    ])

    expect(result.exitCode).toBe(0)
    expect(await currentBranch(repo)).toBe("sprint/example/review")
    expect(await workingTreePorcelain(repo)).toBe("")
    expect((await readState(repo, "example")).tasks.review).toBe("010-task-name")
  })

  test("creates next work when review is occupied", async () => {
    const repo = await createSprintRepo("example", {
      review: "010-task-name",
      next: null,
      approved: [],
    })
    const result = await runCli(repo, [
      "start",
      "--sprint",
      "example",
      "--task",
      "020-task-name",
      "--json",
    ])

    expect(result.exitCode).toBe(0)
    expect(await currentBranch(repo)).toBe("sprint/example/next")
    expect(await branchExists(repo, "sprint/example/next")).toBe(true)
    expect(await workingTreePorcelain(repo)).toBe("")
    expect((await readState(repo, "example")).tasks.next).toBe("020-task-name")
  })

  // Starting a task may reset and check out a sprint branch, so planning must be side-effect free.
  // This catches accidental state writes from the command path used for agent preflight checks.
  test("dry-run leaves review unassigned and branches unmoved", async () => {
    const repo = await createSprintRepo("example", {
      review: null,
      next: null,
      approved: [],
    })
    const beforeState = await readState(repo, "example")

    const result = await runCli(repo, [
      "start",
      "--sprint",
      "example",
      "--task",
      "010-task-name",
      "--dry-run",
      "--json",
    ])
    const start = JSON.parse(result.stdout) as MutationOutput

    expect(result.exitCode).toBe(0)
    expect(start.dryRun).toBe(true)
    expect(start.executed).toBe(false)
    expect(start.gitOperations).toContain("git checkout sprint/example/review")
    expect(await currentBranch(repo)).toBe("main")
    expect(await readState(repo, "example")).toEqual(beforeState)
  })

  // Sprint branches encode task order, not just task identity.
  // Skipping the oldest task would make review stop representing the next human boundary.
  test("refuses to skip the next planned task", async () => {
    const repo = await createSprintRepo("example", {
      review: null,
      next: null,
      approved: [],
    })
    const beforeState = await readState(repo, "example")

    const result = await runCli(repo, [
      "start",
      "--sprint",
      "example",
      "--task",
      "020-task-name",
      "--json",
    ])
    const start = JSON.parse(result.stdout) as MutationOutput

    expect(result.exitCode).toBe(1)
    expect(diagnosticCodes(start)).toContain("task_out_of_order")
    expect(await currentBranch(repo)).toBe("main")
    expect(await readState(repo, "example")).toEqual(beforeState)
  })

  // The workflow deliberately has only two rolling work slots: review and next.
  // A third active task would require the agent to invent branch roles the CLI cannot validate.
  test("refuses a third active task when review and next are occupied", async () => {
    const repo = await createSprintRepo(
      "example",
      {
        review: "010-task-name",
        next: "020-task-name",
        approved: [],
      },
      { createNextBranch: true },
    )
    await fs.writeFile(path.join(repo, "sprints", "example", "030-task-name.md"), "# Task 030\n")
    await commitAll(repo, "add third task")

    const result = await runCli(repo, [
      "start",
      "--sprint",
      "example",
      "--task",
      "030-task-name",
      "--json",
    ])
    const start = JSON.parse(result.stdout) as MutationOutput

    expect(result.exitCode).toBe(1)
    expect(diagnosticCodes(start)).toContain("review_limit_reached")
    expect((await readState(repo, "example")).tasks).toMatchObject({
      review: "010-task-name",
      next: "020-task-name",
    })
  })

  // Branch resets and checkouts are only safe when no unrelated local edits can be stranded.
  // This keeps start from turning a routine transition into manual recovery.
  test("refuses to move sprint branches with a dirty worktree", async () => {
    const repo = await createSprintRepo("example", {
      review: null,
      next: null,
      approved: [],
    })
    await fs.writeFile(path.join(repo, "README.md"), "# Test\ndirty\n")

    const result = await runCli(repo, [
      "start",
      "--sprint",
      "example",
      "--task",
      "010-task-name",
      "--json",
    ])
    const start = JSON.parse(result.stdout) as MutationOutput

    expect(result.exitCode).toBe(1)
    expect(diagnosticCodes(start)).toContain("dirty_worktree")
    expect(await currentBranch(repo)).toBe("main")
    expect((await readState(repo, "example")).tasks.review).toBeNull()
  })
})
