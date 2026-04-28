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
} from "./support"

describe("sprint-branch diff", () => {
  afterEach(cleanupTestRepos)

  test("runs the review diff against approved", async () => {
    const repo = await createSprintRepo("example", {
      review: "010-task-name",
      next: null,
      approved: [],
      finishedUnreviewed: [],
    })
    await git(repo, ["checkout", "sprint/example/review"])
    await fs.writeFile(path.join(repo, "feature.txt"), "reviewed\n")
    await commitAll(repo, "add reviewed work")

    const result = await runCli(repo, ["diff"])

    expect(result.exitCode).toBe(0)
    expect(result.stdout).toContain("diff --git a/feature.txt b/feature.txt")
    expect(result.stdout).toContain("+reviewed")
  })

  // Diff can add safe Git diff display modes while preserving the approved-to-review range.
  // This gives agents changed-file summaries without asking them to compose the Git command.
  test("runs a name-only review diff", async () => {
    const repo = await createSprintRepo("example", {
      review: "010-task-name",
      next: null,
      approved: [],
      finishedUnreviewed: [],
    })
    await git(repo, ["checkout", "sprint/example/review"])
    await fs.writeFile(path.join(repo, "feature.txt"), "reviewed\n")
    await commitAll(repo, "add reviewed work")

    const result = await runCli(repo, ["diff", "--name-only"])

    expect(result.exitCode).toBe(0)
    expect(result.stdout.trim()).toBe("feature.txt")
  })

  test("returns diff output in JSON mode", async () => {
    const repo = await createSprintRepo("example", {
      review: "010-task-name",
      next: null,
      approved: [],
      finishedUnreviewed: [],
    })
    await git(repo, ["checkout", "sprint/example/review"])
    await fs.writeFile(path.join(repo, "feature.txt"), "reviewed\n")
    await commitAll(repo, "add reviewed work")

    const result = await runCli(repo, ["diff", "--json"])
    const diff = JSON.parse(result.stdout) as { output: string; command: string }

    expect(result.exitCode).toBe(0)
    expect(diff.command).toBe("git diff sprint/example/approved...sprint/example/review")
    expect(diff.output).toContain("diff --git a/feature.txt b/feature.txt")
  })

  // The three-dot review diff only has the intended meaning when review descends from approved.
  // If that ancestry is broken, showing a diff could hide that branch recovery is needed first.
  test("refuses when review no longer descends from approved", async () => {
    const repo = await createSprintRepo("example", {
      review: "010-task-name",
      next: null,
      approved: [],
      finishedUnreviewed: [],
    })
    await git(repo, ["checkout", "sprint/example/approved"])
    await fs.writeFile(path.join(repo, "approved.txt"), "approved-only\n")
    await commitAll(repo, "advance approved")

    const result = await runCli(repo, ["diff", "--json"])
    const diff = JSON.parse(result.stdout) as { diagnostics: Array<{ code: string }> }

    expect(result.exitCode).toBe(1)
    expect(diagnosticCodes(diff)).toContain("review_not_based_on_approved")
  })
})
