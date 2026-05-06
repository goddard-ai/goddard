import { afterEach, describe, expect, test } from "bun:test"

import { runFinish } from "../src"
import {
  cleanupTestRepos,
  commitAll,
  createSprintRepo,
  diagnosticCodes,
  git,
  readState,
  runCli,
  writeCompleteReviewReport,
  type MutationOutput,
} from "./support"

describe("sprint-branch finish", () => {
  afterEach(cleanupTestRepos)

  test("fails before the task has a complete Review Report", async () => {
    const repo = await createSprintRepo("example", {
      review: "010-task-name",
      next: null,
      approved: [],
    })

    const result = await runCli(repo, [
      "finish",
      "--sprint",
      "example",
      "--task",
      "010-task-name",
      "--json",
    ])
    const finish = JSON.parse(result.stdout) as MutationOutput

    expect(result.exitCode).toBe(1)
    expect(diagnosticCodes(finish)).toContain("review_report_missing")
    expect((await readState(repo, "example")).tasks.finishedUnreviewed).toEqual([])
  })

  test("marks an active task finished-unreviewed after the Review Report exists", async () => {
    const repo = await createSprintRepo("example", {
      review: "010-task-name",
      next: null,
      approved: [],
    })
    await git(repo, ["checkout", "sprint/example/review"])
    await writeCompleteReviewReport(repo, "example", "010-task-name")
    await commitAll(repo, "add review report")

    const result = await runCli(repo, [
      "finish",
      "--sprint",
      "example",
      "--task",
      "010-task-name",
      "--json",
    ])
    const state = await readState(repo, "example")

    expect(result.exitCode).toBe(0)
    expect(state.tasks.finishedUnreviewed).toEqual(["010-task-name"])
  })

  test("notifies when the review task first becomes ready", async () => {
    const repo = await createSprintRepo("example", {
      review: "010-task-name",
      next: null,
      approved: [],
    })
    await writeCompleteReviewReport(repo, "example", "010-task-name")
    const notifications: Array<{ sprint: string; task: string; reviewBranch: string }> = []

    const finish = await runFinish({
      cwd: repo,
      sprint: "example",
      dryRun: false,
      task: "010-task-name",
      reviewReadyNotifier: (notification) => {
        notifications.push(notification)
      },
    })

    expect(finish.ok).toBe(true)
    expect(notifications).toEqual([
      {
        sprint: "example",
        task: "010-task-name",
        reviewBranch: "sprint/example/review",
      },
    ])
  })

  test("keeps finish successful when the review notification fails", async () => {
    const repo = await createSprintRepo("example", {
      review: "010-task-name",
      next: null,
      approved: [],
    })
    await writeCompleteReviewReport(repo, "example", "010-task-name")

    const finish = await runFinish({
      cwd: repo,
      sprint: "example",
      dryRun: false,
      task: "010-task-name",
      reviewReadyNotifier: () => {
        throw new Error("notification unavailable")
      },
    })

    expect(finish.ok).toBe(true)
    expect((await readState(repo, "example")).tasks.finishedUnreviewed).toEqual(["010-task-name"])
  })
})
