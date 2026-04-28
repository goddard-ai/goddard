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

  test("prints the expected review diff command", async () => {
    const repo = await createSprintRepo("example", {
      review: "010-task-name",
      next: null,
      approved: [],
      finishedUnreviewed: [],
    })
    const result = await runCli(repo, ["diff"])

    expect(result.exitCode).toBe(0)
    expect(result.stdout.trim()).toBe("git diff sprint/example/approved...sprint/example/review")
  })

  test("runs a name-only review diff against the approved branch", async () => {
    const repo = await createSprintRepo("example", {
      review: "010-task-name",
      next: null,
      approved: [],
      finishedUnreviewed: [],
    })
    await git(repo, ["checkout", "sprint/example/review"])
    await fs.writeFile(path.join(repo, "feature.txt"), "reviewed\n")
    await commitAll(repo, "add reviewed work")

    const result = await runCli(repo, ["diff", "--run", "--name-only"])

    expect(result.exitCode).toBe(0)
    expect(result.stdout.trim()).toBe("feature.txt")
  })

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
