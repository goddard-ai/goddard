import { afterEach, describe, expect, test } from "bun:test"

import { cleanupTestRepos, createSprintRepo, currentBranch, readState, runCli } from "./support"

describe("sprint-branch finalize", () => {
  afterEach(cleanupTestRepos)

  test("leaves review checked out for the human merge", async () => {
    const repo = await createSprintRepo("example", {
      review: null,
      next: null,
      approved: ["010-task-name"],
      finishedUnreviewed: [],
    })
    const result = await runCli(repo, ["finalize", "--json"])

    expect(result.exitCode).toBe(0)
    expect(await currentBranch(repo)).toBe("sprint/example/review")
    expect((await readState(repo, "example")).baseBranch).toBe("main")
  })
})
