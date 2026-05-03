import * as fs from "node:fs/promises"
import path from "node:path"
import { afterEach, describe, expect, test } from "bun:test"

import {
  cleanupTestRepos,
  commitAll,
  createBaseRepo,
  createLinkedWorktree,
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

  test("reads untracked task markdown from the recorded sprint worktree", async () => {
    const repo = await createBaseRepo("example")
    await git(repo, ["rm", "-r", "sprints"])
    await commitAll(repo, "remove tracked sprint plans")
    await fs.mkdir(path.join(repo, "sprints", "example"), { recursive: true })
    await fs.writeFile(path.join(repo, "sprints", "example", "010-task-name.md"), "# Task 010\n")
    await fs.writeFile(path.join(repo, "sprints", "example", "020-task-name.md"), "# Task 020\n")
    await writeCompleteReviewReport(repo, "example", "010-task-name")

    expect(
      (await runCli(repo, ["init", "--sprint", "example", "--base", "main", "--json"])).exitCode,
    ).toBe(0)
    expect(
      (await runCli(repo, ["start", "--sprint", "example", "--task", "010-task-name", "--json"]))
        .exitCode,
    ).toBe(0)
    expect(
      (await runCli(repo, ["finish", "--sprint", "example", "--task", "010-task-name", "--json"]))
        .exitCode,
    ).toBe(0)

    const reviewWorktree = await createLinkedWorktree(repo, "sprint/example/review")
    const result = await runCli(reviewWorktree, ["view", "--sprint", "example", "--json"])
    const view = JSON.parse(result.stdout) as {
      ok: boolean
      reviewReport: string
    }
    const reviewTree = await git(reviewWorktree, ["ls-tree", "-r", "--name-only", "HEAD"])

    expect(result.exitCode).toBe(0)
    expect(view.ok).toBe(true)
    expect(view.reviewReport).toContain("## Review Report")
    expect(reviewTree).not.toContain("sprints/example/010-task-name.md")
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
