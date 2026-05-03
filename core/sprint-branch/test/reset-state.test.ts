import * as fs from "node:fs/promises"
import path from "node:path"
import { afterEach, describe, expect, test } from "bun:test"

import {
  branchHead,
  cleanupTestRepos,
  commitAll,
  createBaseRepo,
  createSprintRepo,
  diagnosticCodes,
  git,
  readState,
  runCli,
  type MutationOutput,
} from "./support"

describe("sprint-branch reset-state", () => {
  afterEach(cleanupTestRepos)

  test("creates missing state for the first sprint task without moving branches", async () => {
    const repo = await createBaseRepo("example")
    await git(repo, ["branch", "sprint/example/approved", "main"])
    await git(repo, ["branch", "sprint/example/review", "sprint/example/approved"])
    const approvedHead = await branchHead(repo, "sprint/example/approved")
    const reviewHead = await branchHead(repo, "sprint/example/review")

    const result = await runCli(repo, ["reset-state", "--sprint", "example", "--json"])
    const reset = JSON.parse(result.stdout) as MutationOutput
    const state = await readState(repo, "example")

    expect(result.exitCode).toBe(0)
    expect(reset.executed).toBe(true)
    expect(diagnosticCodes(reset)).toContain("state_file_missing")
    expect(state.tasks).toEqual({
      review: null,
      next: null,
      approved: [],
      finishedUnreviewed: [],
    })
    expect(await branchHead(repo, "sprint/example/approved")).toBe(approvedHead)
    expect(await branchHead(repo, "sprint/example/review")).toBe(reviewHead)
  })

  test("records earlier tasks as approved before the selected target task", async () => {
    const repo = await createSprintRepo(
      "example",
      {
        review: null,
        next: null,
        approved: ["010-task-name"],
      },
      { extraTaskStems: ["030-task-name"] },
    )

    const result = await runCli(repo, [
      "reset-state",
      "--sprint",
      "example",
      "--task",
      "030-task-name",
      "--json",
    ])
    const state = await readState(repo, "example")

    expect(result.exitCode).toBe(0)
    expect(state.tasks).toEqual({
      review: null,
      next: null,
      approved: ["010-task-name", "020-task-name"],
      finishedUnreviewed: [],
    })
  })

  test("refuses to clear active task state unless forced", async () => {
    const repo = await createSprintRepo("example", {
      review: "010-task-name",
      next: null,
      approved: [],
    })
    const before = await readState(repo, "example")

    const result = await runCli(repo, [
      "reset-state",
      "--sprint",
      "example",
      "--task",
      "020-task-name",
      "--json",
    ])
    const reset = JSON.parse(result.stdout) as MutationOutput

    expect(result.exitCode).toBe(1)
    expect(diagnosticCodes(reset)).toContain("active_review_task_recorded")
    expect(await readState(repo, "example")).toEqual(before)

    const forced = await runCli(repo, [
      "reset-state",
      "--sprint",
      "example",
      "--task",
      "020-task-name",
      "--force",
      "--json",
    ])
    expect(forced.exitCode).toBe(0)
    expect((await readState(repo, "example")).tasks).toEqual({
      review: null,
      next: null,
      approved: ["010-task-name"],
      finishedUnreviewed: [],
    })
  })

  test("refuses unrecorded review branch work unless forced", async () => {
    const repo = await createSprintRepo("example", {
      review: null,
      next: null,
      approved: [],
    })
    const before = await readState(repo, "example")

    await git(repo, ["checkout", "sprint/example/review"])
    await fs.writeFile(path.join(repo, "review.txt"), "unrecorded review work\n")
    await commitAll(repo, "add unrecorded review work")

    const result = await runCli(repo, [
      "reset-state",
      "--sprint",
      "example",
      "--task",
      "010-task-name",
      "--json",
    ])
    const reset = JSON.parse(result.stdout) as MutationOutput

    expect(result.exitCode).toBe(1)
    expect(diagnosticCodes(reset)).toContain("review_branch_has_unrecorded_work")
    expect(await readState(repo, "example")).toEqual(before)
  })
})
