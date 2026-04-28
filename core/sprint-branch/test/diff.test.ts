import { afterEach, describe, expect, test } from "bun:test"

import { cleanupTestRepos, createSprintRepo, runCli } from "./support"

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
})
