import * as fs from "node:fs/promises"
import path from "node:path"
import { afterEach, describe, expect, test } from "bun:test"

import {
  cleanupTestRepos,
  commitAll,
  createSprintRepo,
  diagnosticCodes,
  git,
  runCli,
  writeCompleteReviewReport,
} from "./support"

describe("sprint-branch view", () => {
  afterEach(cleanupTestRepos)

  test("prints the current review task approval view", async () => {
    const repo = await createSprintRepo("example", {
      review: "010-task-name",
      next: null,
      approved: [],
      finishedUnreviewed: ["010-task-name"],
    })
    await git(repo, ["checkout", "sprint/example/review"])
    await writeCompleteReviewReport(repo, "example", "010-task-name")
    await commitAll(repo, "add review report")

    const result = await runCli(repo, ["view", "--sprint", "example"])

    expect(result.exitCode).toBe(0)
    expect(result.stdout).toContain("Task: 010-task-name")
    expect(result.stdout).toContain("Review branch: sprint/example/review")
    expect(result.stdout).toContain("Approved comparison branch: sprint/example/approved")
    expect(result.stdout).toContain("Diff: sprint-branch diff --sprint example --stat")
    expect(result.stdout).toContain("## Review Report")
    expect(result.stdout).toContain("### Plain-English Summary")
  })

  test("prints a JSON approval view for an explicit task", async () => {
    const repo = await createSprintRepo("example", {
      review: "010-task-name",
      next: null,
      approved: [],
      finishedUnreviewed: ["010-task-name"],
    })
    await git(repo, ["checkout", "sprint/example/review"])
    await writeCompleteReviewReport(repo, "example", "010-task-name")
    await commitAll(repo, "add review report")

    const result = await runCli(repo, [
      "view",
      "--sprint",
      "example",
      "--task",
      "010-task-name",
      "--json",
    ])
    const view = JSON.parse(result.stdout) as {
      ok: boolean
      task: { id: string }
      reviewReport: string
    }

    expect(result.exitCode).toBe(0)
    expect(view.ok).toBe(true)
    expect(view.task.id).toBe("010-task-name")
    expect(view.reviewReport).toContain("## Review Report")
  })

  test("fails clearly when the Review Report is incomplete", async () => {
    const repo = await createSprintRepo("example", {
      review: "010-task-name",
      next: null,
      approved: [],
      finishedUnreviewed: ["010-task-name"],
    })
    await git(repo, ["checkout", "sprint/example/review"])
    await fs.writeFile(
      path.join(repo, "sprints", "example", "010-task-name.md"),
      "# Task\n\n## Review Report\n\n### Plain-English Summary\n\nDone.\n",
    )
    await commitAll(repo, "add incomplete review report")

    const result = await runCli(repo, ["view", "--sprint", "example", "--json"])
    const view = JSON.parse(result.stdout) as { ok: boolean; diagnostics: Array<{ code: string }> }

    expect(result.exitCode).toBe(1)
    expect(view.ok).toBe(false)
    expect(diagnosticCodes(view)).toContain("review_report_incomplete")
  })
})
